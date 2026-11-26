//! Subtitles: text and bitmap subtitle streams decoded into cues, which the
//! block draws over the video.
//!
//! FFmpeg turns every text format (SRT, WebVTT, MP4 timed text, ASS) into
//! ASS dialogue lines; of most, only their words, line breaks, italics, bold
//! and placement (top, middle or bottom) are kept. An ASS or SSA track's
//! keep how its script draws them too (see [`ass`](super::ass)). Bitmap
//! formats (PGS, VobSub, DVB) become images placed on the picture as
//! fractions of its size.

use std::slice;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use ffmpeg::codec::subtitle::{self as rect, Rect};
use ffmpeg::{Packet, Rational, codec};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;

use super::ass::{self, Dialogue, Script};

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

    /// The karaoke syllables in it, if any, and how many are sung
    /// `since` seconds after it starts.
    fn sung(&self, since: f64) -> Option<usize> {
        let mut syllables = self
            .texts
            .iter()
            .flat_map(|text| &text.spans)
            .filter_map(|span| span.look?.karaoke)
            .peekable();
        syllables.peek()?;
        Some(syllables.filter(|karaoke| karaoke.is_sung(since)).count())
    }

    /// As it shows `since` seconds after it starts: what isn't sung yet in
    /// its secondary color, and without its outline if it's to have none
    /// till then.
    fn singing(&self, since: f64) -> Self {
        let mut texts = self.texts.clone();
        for look in texts
            .iter_mut()
            .flat_map(|text| &mut text.spans)
            .filter_map(|span| span.look.as_mut())
        {
            let Some(karaoke) = look.karaoke.take() else {
                continue;
            };
            if !karaoke.is_sung(since) {
                look.fill = look.secondary;
                if !karaoke.outlined {
                    look.outline &= 0xffff_ff00;
                }
            }
        }
        Self {
            texts,
            bitmaps: self.bitmaps.clone(),
        }
    }
}

/// Lines of text, with `\n` between them.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    /// How each part of `text` is styled, in order, covering all of it.
    pub spans: Vec<Span>,
    pub placement: Placement,
    /// Where and how big an ASS or SSA track's script draws it.
    pub styled: Option<Styled>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    /// In UTF-8 bytes.
    pub len: usize,
    pub italic: bool,
    pub bold: bool,
    /// Its colors and lines, as an ASS or SSA track's script has them.
    pub look: Option<Look>,
}

/// How a part of a line looks, as its script has it. Colors are
/// 0xRRGGBBAA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub fill: u32,
    /// The fill of a karaoke syllable until it's sung.
    pub secondary: u32,
    /// The outline's, or the box's for a boxed line.
    pub outline: u32,
    pub shadow: u32,
    pub underline: bool,
    pub strikeout: bool,
    pub karaoke: Option<Karaoke>,
}

/// A karaoke syllable: when it's sung, and whether it's outlined before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Karaoke {
    /// In milliseconds since its cue started.
    pub at: u32,
    pub outlined: bool,
}

impl Karaoke {
    fn is_sung(self, since: f64) -> bool {
        f64::from(self.at) <= since * 1000.0
    }
}

/// Where and how big a line is drawn, as its script has it; positions and
/// sizes are on its canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct Styled {
    /// The script's width and height.
    pub canvas: (f32, f32),
    /// As a numeric keypad lays it out: 1 bottom left to 9 top right.
    pub alignment: u8,
    /// Left, right, and above or below.
    pub margins: [f32; 3],
    /// Where its alignment's point is, from `\pos`.
    pub position: Option<(f32, f32)>,
    pub font: Arc<str>,
    /// How tall its letters are.
    pub size: f32,
    pub outline: f32,
    pub shadow: f32,
    /// A box behind each row, in its outline color, rather than an
    /// outline.
    pub boxed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Placement {
    #[default]
    Bottom,
    Middle,
    Top,
}

#[derive(Clone)]
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
    /// What it showed last, with so many of its karaoke syllables sung.
    shown: Option<(usize, Arc<CueContent>)>,
}

impl Cue {
    pub fn new(start: f64, end: Option<f64>, content: Option<Arc<CueContent>>) -> Self {
        Self {
            start,
            end,
            content,
            shown: None,
        }
    }

    /// What it shows `since` seconds after it starts: the same, till
    /// another karaoke syllable is sung.
    fn shows(&mut self, since: f64) -> Option<Arc<CueContent>> {
        let content = self.content.as_ref()?;
        let Some(sung) = content.sung(since) else {
            return Some(content.clone());
        };
        match &self.shown {
            Some((shown, showing)) if *shown == sung => Some(showing.clone()),
            _ => {
                let showing = Arc::new(content.singing(since));
                self.shown = Some((sung, showing.clone()));
                Some(showing)
            }
        }
    }
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
            .iter_mut()
            .take_while(|cue| cue.start <= time)
            .filter(|cue| cue.end.is_none_or(|end| end > time))
            .filter_map(|cue| cue.shows(time - cue.start))
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
    /// How an ASS or SSA track's lines are drawn.
    script: Option<Script>,
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
            script: script(&decoder, parameters.id()),
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
                Rect::Ass(line) => {
                    let line = Dialogue::parse(line.get());
                    let script = self.script.as_ref().map(|script| (script, &line));
                    content.texts.extend(ass::parse(line.text, script));
                }
                Rect::Text(text) => content.texts.extend(parse_plain(text.get())),
                Rect::Bitmap(bitmap) => content.bitmaps.extend(self.bitmap(&bitmap)),
                Rect::None(_) => {}
            }
        }
        Some(Cue::new(
            start,
            end.filter(|end| *end > start),
            (!content.is_empty()).then(|| Arc::new(content)),
        ))
    }

    /// The size bitmaps are placed on: the stream's own, else the video's.
    fn canvas(&self) -> Option<(u32, u32)> {
        // SAFETY: the decoder's context is valid while it is. Read through
        // the pointer, never a reference: it holds the file's numbers, which
        // its Rust enums may have no variant for.
        let (width, height) = unsafe {
            let raw = self.decoder.as_ptr();
            ((*raw).width, (*raw).height)
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
            let pixels = pixels(indices, stride, (width, height), palette)?;
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

/// A bitmap's `height` rows of `width` palette indices, `stride` bytes
/// apart in `indices`, as pixels: each its color in `palette`, 0xAARRGGBB,
/// which is BGRA in memory on little-endian machines, the byte order gpui
/// reads images in; an index past it is clear. None if the rows aren't all
/// there.
fn pixels(
    indices: &[u8],
    stride: usize,
    (width, height): (usize, usize),
    palette: &[u32],
) -> Option<Vec<u8>> {
    let needed = stride
        .checked_mul(height.checked_sub(1)?)?
        .checked_add(width)?;
    if width == 0 || stride < width || indices.len() < needed {
        return None;
    }
    let mut pixels = Vec::with_capacity(width * height * 4);
    for row in indices.chunks(stride).take(height) {
        for &index in &row[..width] {
            let color = palette.get(usize::from(index)).copied().unwrap_or(0);
            pixels.extend_from_slice(&color.to_le_bytes());
        }
    }
    Some(pixels)
}

/// The script an ASS or SSA track's header sets out. Other formats' are
/// FFmpeg's own, which say nothing of how a file wants them drawn.
fn script(decoder: &ffmpeg::decoder::Subtitle, id: codec::Id) -> Option<Script> {
    if !matches!(id, codec::Id::ASS | codec::Id::SSA) {
        return None;
    }
    // SAFETY: the decoder's context is valid while it is, and its header,
    // when it has one, `subtitle_header_size` bytes.
    let header = unsafe {
        let raw = decoder.as_ptr();
        let (data, size) = ((*raw).subtitle_header, (*raw).subtitle_header_size);
        if data.is_null() || size <= 0 {
            return None;
        }
        String::from_utf8_lossy(slice::from_raw_parts(data, size as usize)).into_owned()
    };
    Some(Script::parse(&header))
}

/// How a character of a line looks.
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct CharStyle {
    pub italic: bool,
    pub bold: bool,
    pub look: Option<Look>,
}

/// ASS dialogue text, as any text track's reads: override blocks (`{\i1}`)
/// set the style and placement, `\N` breaks lines, `\h` is a hard space;
/// drawings are skipped.
pub fn parse_ass(source: &str) -> Option<Text> {
    ass::parse(source, None)
}

/// Plain text, as it is.
fn parse_plain(source: &str) -> Option<Text> {
    let chars = source.chars().map(|c| (c, CharStyle::default())).collect();
    build_text(chars, Placement::Bottom, None)
}

/// Lines trimmed, blank ones dropped, and runs of one style merged.
pub(super) fn build_text(
    chars: Vec<(char, CharStyle)>,
    placement: Placement,
    styled: Option<Styled>,
) -> Option<Text> {
    let mut lines: Vec<&[(char, CharStyle)]> = chars
        .split(|(c, _)| *c == '\n')
        .map(|line| {
            let blank = |(c, _): &(char, CharStyle)| c.is_whitespace();
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
    let mut add = |c: char, style: CharStyle| {
        text.push(c);
        let span = Span {
            len: c.len_utf8(),
            italic: style.italic,
            bold: style.bold,
            look: style.look,
        };
        match spans.last_mut() {
            Some(last)
                if Span {
                    len: last.len,
                    ..span
                } == *last =>
            {
                last.len += span.len
            }
            _ => spans.push(span),
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
        styled,
    })
}

#[cfg(test)]
#[path = "tests/subtitles.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/subtitles_properties.rs"]
pub(super) mod properties;
