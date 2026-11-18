//! Subtitles: text and bitmap subtitle streams decoded into cues, which the
//! tile draws over the video.
//!
//! FFmpeg turns every text format (SRT, WebVTT, MP4 timed text, ASS) into
//! ASS dialogue lines; only their words, line breaks, italics, bold and
//! placement (top, middle or bottom) are kept. Bitmap formats (PGS, VobSub,
//! DVB) become images placed on the picture as fractions of its size.

use std::slice;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use ffmpeg::codec::subtitle::{self as rect, Rect};
use ffmpeg::{Packet, Rational, codec};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;

const AV_TIME_BASE: f64 = 1_000_000.0;

/// What a cue shows.
#[derive(Default)]
pub struct CueContent {
    pub texts: Vec<Text>,
    pub bitmaps: Vec<Bitmap>,
}

impl CueContent {
    fn is_empty(&self) -> bool {
        self.texts.is_empty() && self.bitmaps.is_empty()
    }

    /// The same words or pictures in the same places, as when a cue is
    /// decoded again.
    fn same_as(&self, other: &Self) -> bool {
        self.texts == other.texts
            && self.bitmaps.len() == other.bitmaps.len()
            && self
                .bitmaps
                .iter()
                .zip(&other.bitmaps)
                .all(|(a, b)| a.rect == b.rect && a.image.size(0) == b.image.size(0))
    }
}

/// Lines of text, with `\n` between them.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    /// How each part of `text` is styled, in order, covering all of it.
    pub spans: Vec<Span>,
    pub placement: Placement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    /// In UTF-8 bytes.
    pub len: usize,
    pub italic: bool,
    pub bold: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Placement {
    #[default]
    Bottom,
    Middle,
    Top,
}

pub struct Bitmap {
    pub image: Arc<RenderImage>,
    /// Left, top, width and height, as fractions of the picture's size.
    pub rect: [f32; 4],
}

/// A decoded subtitle and when it shows, in seconds of the media's timeline.
pub struct Cue {
    pub start: f64,
    /// `None` until the next cue starts, which is how bitmap subtitles end.
    pub end: Option<f64>,
    /// `None` for a cue that only clears the screen.
    pub content: Option<Arc<CueContent>>,
}

/// Cues are kept this long after they end, and from this far ahead, so
/// going back (a frame, or a few seconds) shows the ones just over without
/// waiting for them to be read again, and moving subtitles later brings them
/// back. A seek may go back to a keyframe after a cue started, which then
/// isn't read again at all.
const KEEP: f64 = 60.0;
/// Cues starting this close together, with the same content, are one cue
/// decoded twice, in seconds.
const SAME_START: f64 = 0.001;

/// Cues decoded around the playhead, for the subtitle track playing. Seeks
/// keep them.
#[derive(Default)]
pub struct CueQueue {
    /// How much later than their times cues show, in seconds (negative:
    /// earlier).
    pub delay: f64,
    /// In start order.
    cues: Vec<Cue>,
}

impl CueQueue {
    /// Forget every cue, for another track.
    pub fn clear(&mut self) {
        self.cues.clear();
    }

    /// The playhead jumped from `from` to `to`. A cue still waiting for an
    /// end is only known to go on as far as the playhead has been with it,
    /// so it ends there if `to` is further.
    pub fn seeked(&mut self, from: f64, to: f64) {
        let (from, to) = (self.cue_time(from), self.cue_time(to));
        for cue in &mut self.cues {
            let known = cue.start.max(from);
            if cue.end.is_none() && to > known {
                cue.end = Some(known);
            }
        }
    }

    /// Add a cue. Cues still waiting for an end end where it starts. One
    /// decoded again (after a seek back) is kept once, with the end it has
    /// now.
    pub fn push(&mut self, cue: Cue) {
        for open in &mut self.cues {
            if open.end.is_none() && open.start < cue.start {
                open.end = Some(cue.start);
            }
        }
        let Some(content) = &cue.content else {
            return;
        };
        let same = self.cues.iter_mut().find(|known| {
            (known.start - cue.start).abs() < SAME_START
                && known.content.as_ref().is_some_and(|c| c.same_as(content))
        });
        match same {
            Some(known) => known.end = cue.end,
            None => {
                let at = self.cues.partition_point(|c| c.start <= cue.start);
                self.cues.insert(at, cue);
            }
        }
    }

    /// The time on the cues' own timeline when the playhead is at `time`.
    pub fn cue_time(&self, time: f64) -> f64 {
        time - self.delay
    }

    /// Forget cues that have been over for a while with the playhead at
    /// `time`, or that are a long way ahead of it.
    pub fn prune(&mut self, time: f64) {
        let time = self.cue_time(time);
        self.cues
            .retain(|cue| cue.end.is_none_or(|end| end > time - KEEP) && cue.start < time + KEEP);
    }

    /// The cues showing with the playhead at `time`, oldest first.
    pub fn showing(&mut self, time: f64) -> Vec<Arc<CueContent>> {
        self.prune(time);
        let time = self.cue_time(time);
        self.cues
            .iter()
            .take_while(|cue| cue.start <= time)
            .filter(|cue| cue.end.is_none_or(|end| end > time))
            .filter_map(|cue| cue.content.clone())
            .collect()
    }
}

/// A decoded `AVSubtitle`, freed when dropped.
struct Decoded(ffmpeg::Subtitle);

impl Drop for Decoded {
    fn drop(&mut self) {
        // SAFETY: frees what the decoder allocated for this subtitle (and
        // nothing if it allocated nothing); the struct is zeroed afterwards.
        unsafe { ffmpeg::ffi::avsubtitle_free(self.0.as_mut_ptr()) };
    }
}

pub struct SubtitleDecoder {
    pub index: usize,
    decoder: ffmpeg::decoder::Subtitle,
    time_base: f64,
    /// The picture size bitmaps are placed on when the stream doesn't say:
    /// the video's coded size.
    canvas: Option<(u32, u32)>,
}

impl SubtitleDecoder {
    /// A decoder for a subtitle stream. Text in another `encoding` than
    /// UTF-8 (an iconv name) is converted, where FFmpeg can.
    pub fn new(
        index: usize,
        parameters: &codec::Parameters,
        time_base: Rational,
        canvas: Option<(u32, u32)>,
        encoding: Option<&str>,
    ) -> Result<Self> {
        let open = |encoding: Option<&str>| -> Result<ffmpeg::decoder::Subtitle> {
            let context = codec::context::Context::from_parameters(parameters.clone())?;
            let mut decoder = context.decoder();
            // Lets FFmpeg take a cue's end from its packet's duration.
            decoder.set_packet_time_base(time_base);
            let codec = ffmpeg::decoder::find(parameters.id()).context("no decoder")?;
            let mut options = ffmpeg::Dictionary::new();
            if let Some(encoding) = encoding {
                options.set("sub_charenc", encoding);
            }
            Ok(decoder.open_as_with(codec, options)?.subtitle()?)
        };
        let decoder = match encoding.map(|encoding| (encoding, open(Some(encoding)))) {
            Some((_, Ok(decoder))) => Ok(decoder),
            Some((encoding, Err(err))) => {
                log::warn!("can't convert subtitles from {encoding}: {err:#}");
                open(None)
            }
            None => open(None),
        }
        .context("opening the subtitle decoder")?;
        Ok(Self {
            index,
            decoder,
            time_base: f64::from(time_base),
            canvas,
        })
    }

    pub fn reset(&mut self) {
        self.decoder.flush();
    }

    /// Decode a packet into a cue, timed on the media's timeline (which
    /// starts at `offset` in the file's timestamps).
    pub fn decode(&mut self, packet: &Packet, offset: f64) -> Option<Cue> {
        let mut subtitle = Decoded(ffmpeg::Subtitle::new());
        match self.decoder.decode(packet, &mut subtitle.0) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(err) => {
                log::debug!("subtitle decode error: {err}");
                return None;
            }
        }
        let subtitle = &subtitle.0;
        let base = subtitle
            .pts()
            .map(|pts| pts as f64 / AV_TIME_BASE)
            .or_else(|| packet.pts().map(|pts| pts as f64 * self.time_base))?
            - offset;
        let start = base + f64::from(subtitle.start()) / 1000.0;
        let end = match subtitle.end() {
            0 | u32::MAX => {
                (packet.duration() > 0).then(|| base + packet.duration() as f64 * self.time_base)
            }
            end => Some(base + f64::from(end) / 1000.0),
        };

        let mut content = CueContent::default();
        for rect in subtitle.rects() {
            match rect {
                Rect::Ass(ass) => content.texts.extend(parse_ass(dialogue_text(ass.get()))),
                Rect::Text(text) => content.texts.extend(parse_plain(text.get())),
                Rect::Bitmap(bitmap) => content.bitmaps.extend(self.bitmap(&bitmap)),
                Rect::None(_) => {}
            }
        }
        Some(Cue {
            start,
            end: end.filter(|end| *end > start),
            content: (!content.is_empty()).then(|| Arc::new(content)),
        })
    }

    /// The size bitmaps are placed on: the stream's own, else the video's.
    fn canvas(&self) -> Option<(u32, u32)> {
        // SAFETY: the decoder's context is valid while it is.
        let (width, height) = unsafe {
            let context = &*self.decoder.as_ptr();
            (context.width, context.height)
        };
        if width > 0 && height > 0 {
            Some((width as u32, height as u32))
        } else {
            self.canvas
        }
    }

    fn bitmap(&self, bitmap: &rect::Bitmap) -> Option<Bitmap> {
        let (canvas_width, canvas_height) = self.canvas()?;
        // SAFETY: the rect belongs to the subtitle being read. Its `data[0]`
        // holds `h` rows of `w` palette indices, `linesize[0]` bytes apart,
        // and `data[1]` its `nb_colors` palette entries.
        let (pixels, width, height, x, y) = unsafe {
            let raw = &*bitmap.as_ptr();
            let (width, height) = (raw.w.max(0) as usize, raw.h.max(0) as usize);
            let stride = raw.linesize[0].max(0) as usize;
            if width == 0 || height == 0 || stride < width {
                return None;
            }
            if raw.data[0].is_null() || raw.data[1].is_null() {
                return None;
            }
            let indices = slice::from_raw_parts(raw.data[0], stride * (height - 1) + width);
            let palette = slice::from_raw_parts(
                raw.data[1].cast::<u32>(),
                raw.nb_colors.clamp(0, 256) as usize,
            );
            let mut pixels = Vec::with_capacity(width * height * 4);
            for row in indices.chunks(stride).take(height) {
                for &index in &row[..width] {
                    // 0xAARRGGBB, which is BGRA in memory on little-endian
                    // machines, the byte order gpui reads images in.
                    let color = palette.get(usize::from(index)).copied().unwrap_or(0);
                    pixels.extend_from_slice(&color.to_le_bytes());
                }
            }
            (pixels, width as u32, height as u32, raw.x, raw.y)
        };
        let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
        let (cw, ch) = (canvas_width as f32, canvas_height as f32);
        Some(Bitmap {
            image: Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])),
            rect: [
                x as f32 / cw,
                y as f32 / ch,
                width as f32 / cw,
                height as f32 / ch,
            ],
        })
    }
}

/// The text of an ASS dialogue line as FFmpeg gives it:
/// `ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text` (older
/// versions gave the whole `Dialogue:` line, with its times).
fn dialogue_text(line: &str) -> &str {
    let fields = if line.starts_with("Dialogue:") { 10 } else { 9 };
    line.splitn(fields, ',').nth(fields - 1).unwrap_or(line)
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Style {
    italic: bool,
    bold: bool,
}

/// ASS dialogue text: override blocks (`{\i1}`) set the style and
/// placement, `\N` breaks lines, `\h` is a hard space; drawings are skipped.
pub fn parse_ass(source: &str) -> Option<Text> {
    let mut chars = Vec::new();
    let mut style = Style::default();
    let mut placement = None;
    let mut drawing = false;
    let mut rest = source;
    while let Some(c) = rest.chars().next() {
        if c == '{'
            && let Some(close) = rest.find('}')
        {
            for tag in rest[1..close].split('\\').skip(1) {
                let name_len = tag
                    .find(|c: char| !c.is_ascii_alphabetic())
                    .unwrap_or(tag.len());
                let (name, value) = tag.split_at(name_len);
                let number = value.trim().parse::<u32>().ok();
                match (name, number) {
                    ("i", n) => style.italic = n.is_some_and(|n| n != 0),
                    ("b", n) => style.bold = n.is_some_and(|n| n == 1 || n >= 600),
                    ("r", _) => style = Style::default(),
                    ("p", n) => drawing = n.is_some_and(|n| n != 0),
                    ("an", Some(n)) if placement.is_none() => {
                        placement = Some(match n {
                            7..=9 => Placement::Top,
                            4..=6 => Placement::Middle,
                            _ => Placement::Bottom,
                        });
                    }
                    ("a", Some(n)) if placement.is_none() => {
                        placement = Some(match n {
                            5..=7 => Placement::Top,
                            9..=11 => Placement::Middle,
                            _ => Placement::Bottom,
                        });
                    }
                    _ => {}
                }
            }
            rest = &rest[close + 1..];
            continue;
        }
        let (text, len) = match rest.as_bytes() {
            [b'\\', b'N' | b'n', ..] => ('\n', 2),
            [b'\\', b'h', ..] => ('\u{a0}', 2),
            _ => (c, c.len_utf8()),
        };
        if !drawing {
            chars.push((text, style));
        }
        rest = &rest[len..];
    }
    build_text(chars, placement.unwrap_or_default())
}

/// Plain text, as it is.
fn parse_plain(source: &str) -> Option<Text> {
    let chars = source.chars().map(|c| (c, Style::default())).collect();
    build_text(chars, Placement::Bottom)
}

/// Lines trimmed, blank ones dropped, and runs of one style merged.
fn build_text(chars: Vec<(char, Style)>, placement: Placement) -> Option<Text> {
    let mut lines: Vec<&[(char, Style)]> = chars
        .split(|(c, _)| *c == '\n')
        .map(|line| {
            let blank = |(c, _): &(char, Style)| c.is_whitespace();
            let start = line.iter().position(|c| !blank(c)).unwrap_or(line.len());
            let end = line
                .iter()
                .rposition(|c| !blank(c))
                .map_or(start, |i| i + 1);
            &line[start..end]
        })
        .collect();
    lines.retain(|line| !line.is_empty());
    if lines.is_empty() {
        return None;
    }

    let mut text = String::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut add = |c: char, style: Style| {
        text.push(c);
        match spans.last_mut() {
            Some(span) if span.italic == style.italic && span.bold == style.bold => {
                span.len += c.len_utf8();
            }
            _ => spans.push(Span {
                len: c.len_utf8(),
                italic: style.italic,
                bold: style.bold,
            }),
        }
    };
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            // Styled like the line before, so it doesn't split a run.
            add('\n', line[0].1);
        }
        for &(c, style) in *line {
            add(c, style);
        }
    }
    Some(Text {
        text,
        spans,
        placement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styled(text: &Text) -> Vec<(&str, bool, bool)> {
        let mut at = 0;
        text.spans
            .iter()
            .map(|span| {
                let part = &text.text[at..at + span.len];
                at += span.len;
                (part, span.italic, span.bold)
            })
            .collect()
    }

    #[test]
    fn reads_dialogue_text() {
        assert_eq!(
            dialogue_text("12,0,Default,,0,0,0,,Hello, world"),
            "Hello, world"
        );
        assert_eq!(
            dialogue_text("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hi, there"),
            "Hi, there"
        );
    }

    #[test]
    fn parses_ass_markup() {
        let text = parse_ass(r"{\i1}Hello{\i0}\Nworld\hagain").unwrap();
        assert_eq!(text.text, "Hello\nworld\u{a0}again");
        assert_eq!(
            styled(&text),
            [("Hello", true, false), ("\nworld\u{a0}again", false, false)]
        );
        assert_eq!(text.placement, Placement::Bottom);

        let text = parse_ass(r"{\an8\b1}Top{\b0} line").unwrap();
        assert_eq!(text.placement, Placement::Top);
        assert_eq!(
            styled(&text),
            [("Top", false, true), (" line", false, false)]
        );

        // Font tags and positions are ignored, drawings are skipped, blank
        // lines and spaces around lines are dropped.
        let text = parse_ass(r"{\fnArial\fs20\bord2\pos(10,20)} One \N\N{\p1}m 0 0 l 1 1{\p0}Two ")
            .unwrap();
        assert_eq!(text.text, "One\nTwo");
        assert_eq!(parse_ass(r"{\p1}m 0 0 l 10 10"), None);
        assert_eq!(parse_ass(r" \N "), None);
        // An unclosed brace is just text.
        assert_eq!(parse_ass("a { b").unwrap().text, "a { b");
    }

    fn cue(start: f64, end: Option<f64>) -> Cue {
        Cue {
            start,
            end,
            content: Some(Arc::new(CueContent {
                texts: parse_plain("x").into_iter().collect(),
                bitmaps: Vec::new(),
            })),
        }
    }

    #[test]
    fn shows_cues_in_their_time() {
        let mut queue = CueQueue::default();
        queue.push(cue(1.0, Some(3.0)));
        queue.push(cue(2.0, Some(4.0)));
        assert_eq!(queue.showing(0.5).len(), 0);
        assert_eq!(queue.showing(1.5).len(), 1);
        assert_eq!(queue.showing(2.5).len(), 2);
        assert_eq!(queue.showing(3.5).len(), 1);
        assert_eq!(queue.showing(4.0).len(), 0);
        assert_eq!(queue.cues.len(), 2, "kept a while, to go back to");
        assert_eq!(queue.showing(4.0 + KEEP).len(), 0);
        assert!(queue.cues.is_empty());
    }

    #[test]
    fn shows_cues_later_or_earlier_by_the_delay() {
        let mut queue = CueQueue::default();
        queue.push(cue(1.0, Some(3.0)));
        queue.delay = 1.5;
        assert_eq!(queue.showing(2.0).len(), 0);
        assert_eq!(queue.showing(2.5).len(), 1);
        assert_eq!(queue.showing(4.4).len(), 1);
        assert_eq!(queue.showing(4.5).len(), 0);
        // Back earlier, it shows again: ended cues are kept a while.
        queue.delay = -0.5;
        assert_eq!(queue.showing(0.4).len(), 0);
        assert_eq!(queue.showing(0.5).len(), 1);
        assert_eq!(queue.showing(2.5).len(), 0);
        // Clearing for another track keeps the delay.
        queue.clear();
        assert_eq!(queue.delay, -0.5);
    }

    #[test]
    fn open_cues_end_at_the_next_one() {
        let mut queue = CueQueue::default();
        queue.push(cue(1.0, None));
        assert_eq!(queue.showing(100.0).len(), 1);
        queue.clear();
        queue.push(cue(1.0, None));
        queue.push(cue(5.0, None));
        let first = queue.showing(2.0);
        assert_eq!(first.len(), 1);
        let second = queue.showing(5.0);
        assert_eq!(second.len(), 1);
        assert!(!Arc::ptr_eq(&first[0], &second[0]));
        // A cue with nothing in it clears the screen.
        queue.push(Cue {
            start: 6.0,
            end: None,
            content: None,
        });
        assert_eq!(queue.showing(5.5).len(), 1);
        assert_eq!(queue.showing(6.0).len(), 0);
    }

    #[test]
    fn a_cue_decoded_again_is_kept_once() {
        let mut queue = CueQueue::default();
        queue.push(cue(1.0, Some(3.0)));
        queue.push(cue(1.0, Some(3.0)));
        assert_eq!(queue.showing(2.0).len(), 1);
        // Something else starting then is another cue.
        queue.push(Cue {
            start: 1.0,
            end: Some(3.0),
            content: Some(Arc::new(CueContent {
                texts: parse_plain("y").into_iter().collect(),
                bitmaps: Vec::new(),
            })),
        });
        assert_eq!(queue.showing(2.0).len(), 2);
    }

    #[test]
    fn open_cues_go_on_only_as_far_as_the_playhead_saw() {
        let mut queue = CueQueue::default();
        queue.push(cue(5.0, None));
        // Back a frame from 6: still showing, and from where it started.
        queue.seeked(6.0, 5.96);
        assert_eq!(queue.showing(5.96).len(), 1);
        assert_eq!(queue.showing(5.0).len(), 1);
        // Read again from before it started, it is still one cue.
        queue.push(cue(5.0, None));
        assert_eq!(queue.showing(5.5).len(), 1);
        // Past where it was seen, it isn't known to show, so it doesn't.
        queue.seeked(6.0, 20.0);
        assert_eq!(queue.showing(20.0).len(), 0);
        assert_eq!(queue.showing(5.5).len(), 1);
    }
}
