//! Subtitles for a cast: every cue of a text track in the media, or of a
//! subtitle file, read once and written as WebVTT (for HLS and Chromecast)
//! or SRT (for DLNA TVs). Italic and bold are kept, and in WebVTT where a
//! cue goes: at the top, in the middle or at the bottom, to the left, in
//! the centre or to the right, as its ASS style or tags put it. Picture
//! subtitles (PGS, DVD) have no text: they go drawn into the
//! video, where it is re-encoded (see `overlay`).

use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ffmpeg::codec;
use ffmpeg::codec::subtitle::Rect;
use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;

use crate::Error;
use crate::media::{self, Source};
use crate::segments::set_discard;

/// A cue's end when its track doesn't say, in seconds after its start.
const UNKNOWN_LENGTH: f64 = 4.0;

/// Where a cast's subtitles come from.
#[derive(Clone, PartialEq, Debug)]
pub enum SubtitleSource {
    /// A subtitle track of the cast's own media, by stream index: text
    /// ones are served beside the media, picture ones drawn into its video
    /// where that is re-encoded.
    Track(usize),
    /// A subtitle file (SRT, ASS, WebVTT and the like), with its text's
    /// encoding by iconv name when it is neither UTF-8 nor UTF-16.
    File {
        path: PathBuf,
        encoding: Option<String>,
    },
}

/// A run of text in one style; `\n` breaks lines.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Span {
    pub text: String,
    pub italic: bool,
    pub bold: bool,
}

/// A cue, timed from the start of the media.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Cue {
    pub start: f64,
    pub end: f64,
    pub spans: Vec<Span>,
    pub placement: Placement,
}

/// Where a cue goes on the picture, in thirds of it. Bottom centre, where
/// a player puts any it's told nothing of, unless said otherwise.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Placement {
    pub vertical: Vertical,
    pub horizontal: Horizontal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Vertical {
    Top,
    Middle,
    #[default]
    Bottom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Horizontal {
    Left,
    #[default]
    Center,
    Right,
}

impl Placement {
    /// ASS's alignment, as on a numeric keypad: 1 to 3 along the bottom,
    /// left to right, 4 to 6 the middle, 7 to 9 the top.
    fn numpad(alignment: u32) -> Option<Self> {
        let index = alignment.checked_sub(1).filter(|&index| index < 9)?;
        Some(Self {
            vertical: [Vertical::Bottom, Vertical::Middle, Vertical::Top][index as usize / 3],
            horizontal: [Horizontal::Left, Horizontal::Center, Horizontal::Right]
                [index as usize % 3],
        })
    }

    /// SSA's alignment: 1 to 3 left to right along the bottom, 4 more for
    /// the top, 8 more for the middle.
    fn legacy(alignment: u32) -> Option<Self> {
        let horizontal = match alignment & 3 {
            1 => Horizontal::Left,
            2 => Horizontal::Center,
            3 => Horizontal::Right,
            _ => return None,
        };
        let vertical = match alignment & !3 {
            0 => Vertical::Bottom,
            4 => Vertical::Top,
            8 => Vertical::Middle,
            _ => return None,
        };
        Some(Self {
            vertical,
            horizontal,
        })
    }

    /// Where a point `x`, `y` is on a `width` × `height` picture.
    fn at(x: f64, y: f64, (width, height): (f64, f64)) -> Self {
        let third = |at: f64, of: f64| (at / of * 3.0).floor().clamp(0.0, 2.0) as usize;
        Self {
            vertical: [Vertical::Top, Vertical::Middle, Vertical::Bottom][third(y, height)],
            horizontal: [Horizontal::Left, Horizontal::Center, Horizontal::Right][third(x, width)],
        }
    }

    /// The WebVTT cue settings that put a cue here, each after a space: a
    /// line from the top or the middle, text from near either side.
    fn webvtt(self) -> String {
        let line = match self.vertical {
            Vertical::Top => " line:0",
            Vertical::Middle => " line:50%,center",
            Vertical::Bottom => "",
        };
        let side = match self.horizontal {
            Horizontal::Left => " position:5% align:left",
            Horizontal::Center => "",
            Horizontal::Right => " position:95% align:right",
        };
        format!("{line}{side}")
    }
}

/// A track's ASS styles' placements, by name, and the size of the picture
/// its positions are on, from its header.
#[derive(Debug, PartialEq)]
struct Styles {
    placements: HashMap<String, Placement>,
    size: (f64, f64),
}

impl Styles {
    fn of(header: &str) -> Self {
        let mut placements = HashMap::new();
        let (mut width, mut height) = (None, None);
        let mut legacy = false;
        let mut columns: Option<(usize, usize)> = None;
        for line in header.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                legacy = line.eq_ignore_ascii_case("[V4 Styles]");
                columns = None;
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "PlayResX" => width = value.parse::<f64>().ok().filter(|&w| w > 0.0),
                "PlayResY" => height = value.parse::<f64>().ok().filter(|&h| h > 0.0),
                "Format" => {
                    let names: Vec<_> = value.split(',').map(str::trim).collect();
                    let column =
                        |name: &str| names.iter().position(|n| n.eq_ignore_ascii_case(name));
                    columns = column("Name").zip(column("Alignment"));
                }
                "Style" => {
                    let Some((name, alignment)) = columns else {
                        continue;
                    };
                    let fields: Vec<_> = value.split(',').map(str::trim).collect();
                    let placement =
                        fields
                            .get(alignment)
                            .and_then(|a| a.parse().ok())
                            .and_then(|a| {
                                if legacy {
                                    Placement::legacy(a)
                                } else {
                                    Placement::numpad(a)
                                }
                            });
                    if let (Some(name), Some(placement)) = (fields.get(name), placement) {
                        placements.insert((*name).to_owned(), placement);
                    }
                }
                _ => {}
            }
        }
        // As players take a size the header leaves out: 384 × 288, or 4:3
        // to the side it gives.
        let size = match (width, height) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) => (width, width * 3.0 / 4.0),
            (None, Some(height)) => (height * 4.0 / 3.0, height),
            (None, None) => (384.0, 288.0),
        };
        Self { placements, size }
    }

    /// Where an ASS event goes: where its tags put it, else its style.
    fn placement(&self, line: &str) -> Placement {
        let style = dialogue_style(line).trim_start_matches('*');
        let style = self
            .placements
            .get(style)
            .or_else(|| self.placements.get("Default"))
            .copied()
            .unwrap_or_default();
        let tags = Tags::of(dialogue_text(line));
        match (tags.position, tags.alignment) {
            (Some((x, y)), _) => Placement::at(x, y, self.size),
            (None, Some(alignment)) => alignment,
            (None, None) => style,
        }
    }
}

/// What an ASS event's override tags say of where it goes: the first of
/// each, as players take them.
#[derive(Debug, Default, PartialEq)]
struct Tags {
    alignment: Option<Placement>,
    position: Option<(f64, f64)>,
}

impl Tags {
    fn of(text: &str) -> Self {
        let mut tags = Self::default();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            for tag in rest[open + 1..open + close].split('\\').skip(1) {
                let tag = tag.trim();
                if let Some(value) = tag.strip_prefix("an") {
                    if tags.alignment.is_none() {
                        tags.alignment = value.trim().parse().ok().and_then(Placement::numpad);
                    }
                } else if let Some(value) = tag.strip_prefix('a') {
                    if tags.alignment.is_none() {
                        tags.alignment = value.trim().parse().ok().and_then(Placement::legacy);
                    }
                } else if let Some(arguments) = tag
                    .strip_prefix("pos(")
                    .or_else(|| tag.strip_prefix("move("))
                    && tags.position.is_none()
                {
                    let mut numbers = arguments
                        .trim_end_matches(')')
                        .split(',')
                        .map(|n| n.trim().parse::<f64>());
                    if let (Some(Ok(x)), Some(Ok(y))) = (numbers.next(), numbers.next()) {
                        tags.position = Some((x, y));
                    }
                }
            }
            rest = &rest[open + close + 1..];
        }
        tags
    }
}

/// Read every cue of `subtitles`, for media from `source`.
pub(crate) fn read(
    subtitles: &SubtitleSource,
    source: &Source,
    stop: Arc<AtomicBool>,
) -> Result<Vec<Cue>, Error> {
    let (mut input, encoding) = match subtitles {
        SubtitleSource::Track(_) => {
            if source.is_live() {
                return Err(Error::Unsupported(
                    "a live stream's subtitle track can't be read ahead".into(),
                ));
            }
            (media::open(source, stop)?, None)
        }
        SubtitleSource::File { path, encoding } => (
            ffmpeg_safe::input_file(path)
                .map_err(|err| Error::Source(format!("can't open {}: {err}", path.display())))?,
            encoding.as_deref(),
        ),
    };
    let index = match subtitles {
        SubtitleSource::Track(index) => *index,
        SubtitleSource::File { .. } => input
            .streams()
            .best(Type::Subtitle)
            .map(|stream| stream.index())
            .ok_or_else(|| Error::Unsupported("the file has no subtitles in it".into()))?,
    };
    // An embedded track is timed from the start of the media it is in; a
    // file from zero, where the media starts too.
    let offset = match subtitles {
        SubtitleSource::Track(_) => {
            // SAFETY: the context is open while `input` is.
            let start = unsafe { (*input.as_ptr()).start_time };
            if start == ffmpeg::ffi::AV_NOPTS_VALUE {
                0.0
            } else {
                start as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
            }
        }
        SubtitleSource::File { .. } => 0.0,
    };
    let (parameters, time_base) = {
        let stream = input
            .stream(index)
            .ok_or_else(|| Error::Unsupported(format!("there is no stream {index}")))?;
        if stream.parameters().medium() != Type::Subtitle {
            return Err(Error::Unsupported(format!(
                "stream {index} isn't subtitles"
            )));
        }
        (stream.parameters(), stream.time_base())
    };
    let mut decoder = open_decoder(&parameters, time_base, encoding)?;
    let styles = Styles::of(&header(&decoder));
    set_discard(&mut input, Some(index));

    let time_base = f64::from(time_base);
    let mut cues = Vec::new();
    for (stream, packet) in input.packets() {
        if stream.index() != index {
            continue;
        }
        let mut subtitle = Decoded(ffmpeg::Subtitle::new());
        match decoder.decode(&packet, &mut subtitle.0) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => {
                log::debug!("a cue didn't decode: {err}");
                continue;
            }
        }
        let subtitle = &subtitle.0;
        let Some(base) = subtitle
            .pts()
            .map(|pts| pts as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE))
            .or_else(|| packet.pts().map(|pts| pts as f64 * time_base))
        else {
            continue;
        };
        let base = base - offset;
        let start = base + f64::from(subtitle.start()) / 1000.0;
        let end = match subtitle.end() {
            0 | u32::MAX if packet.duration() > 0 => base + packet.duration() as f64 * time_base,
            0 | u32::MAX => start + UNKNOWN_LENGTH,
            end => base + f64::from(end) / 1000.0,
        };
        let spans = spans_of(subtitle);
        if !spans.is_empty() && end > start {
            cues.push(Cue {
                start,
                end,
                spans,
                placement: placement_of(subtitle, &styles),
            });
        }
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(cues)
}

/// A decoded subtitle's text, each of its parts on lines of its own.
fn spans_of(subtitle: &ffmpeg::Subtitle) -> Vec<Span> {
    let mut spans = Vec::new();
    for rect in subtitle.rects() {
        let more = match rect {
            Rect::Ass(ass) => from_ass(dialogue_text(&ffmpeg_safe::ass_line(&ass))),
            Rect::Text(text) => vec![Span {
                text: ffmpeg_safe::plain_text(&text).trim().to_owned(),
                ..Span::default()
            }],
            Rect::Bitmap(_) | Rect::None(_) => Vec::new(),
        };
        if !spans.is_empty() && !more.is_empty() {
            push(&mut spans, "\n", Span::default());
        }
        for span in more {
            let style = Span {
                text: String::new(),
                ..span
            };
            push(&mut spans, &span.text, style);
        }
    }
    tidy(spans)
}

/// `cues`, each shown `delay` seconds after its time (negative: before);
/// those that would be over before the start go.
pub(crate) fn delayed(cues: &[Cue], delay: f64) -> Vec<Cue> {
    cues.iter()
        .filter(|cue| cue.end + delay > 0.0)
        .map(|cue| Cue {
            start: (cue.start + delay).max(0.0),
            end: cue.end + delay,
            ..cue.clone()
        })
        .collect()
}

/// Where a decoded subtitle goes: where its first ASS event does.
fn placement_of(subtitle: &ffmpeg::Subtitle, styles: &Styles) -> Placement {
    subtitle
        .rects()
        .find_map(|rect| match rect {
            Rect::Ass(ass) => Some(styles.placement(&ffmpeg_safe::ass_line(&ass))),
            _ => None,
        })
        .unwrap_or_default()
}

/// The ASS header a subtitle decoder makes its events for: the track's
/// own, or FFmpeg's for text it turns into ASS.
fn header(decoder: &ffmpeg::decoder::Subtitle) -> String {
    // SAFETY: the decoder is open, and its header, where it has one, is
    // `subtitle_header_size` bytes it owns.
    unsafe {
        let context = decoder.as_ptr();
        let (data, size) = ((*context).subtitle_header, (*context).subtitle_header_size);
        match usize::try_from(size) {
            Ok(size) if !data.is_null() && size > 0 => {
                String::from_utf8_lossy(std::slice::from_raw_parts(data, size)).into_owned()
            }
            _ => String::new(),
        }
    }
}

/// A decoded `AVSubtitle`, freed when dropped.
pub(crate) struct Decoded(pub ffmpeg::Subtitle);

impl Drop for Decoded {
    fn drop(&mut self) {
        // SAFETY: frees what the decoder allocated for this subtitle (and
        // nothing if it allocated nothing).
        unsafe { ffmpeg::ffi::avsubtitle_free(self.0.as_mut_ptr()) };
    }
}

/// A decoder for the track, converting its text from `encoding` where
/// FFmpeg can.
fn open_decoder(
    parameters: &codec::Parameters,
    time_base: ffmpeg::Rational,
    encoding: Option<&str>,
) -> Result<ffmpeg::decoder::Subtitle, Error> {
    let open = |encoding: Option<&str>| -> Result<ffmpeg::decoder::Subtitle, ffmpeg::Error> {
        let context = codec::context::Context::from_parameters(parameters.clone())?;
        let mut decoder = context.decoder();
        // Lets FFmpeg take a cue's end from its packet's duration.
        decoder.set_packet_time_base(time_base);
        let codec = ffmpeg::decoder::find(parameters.id()).ok_or(ffmpeg::Error::DecoderNotFound)?;
        let mut options = ffmpeg::Dictionary::new();
        if let Some(encoding) = encoding {
            options.set("sub_charenc", encoding);
        }
        decoder.open_as_with(codec, options)?.subtitle()
    };
    match encoding.map(|encoding| (encoding, open(Some(encoding)))) {
        Some((_, Ok(decoder))) => Ok(decoder),
        Some((encoding, Err(err))) => {
            log::warn!("can't convert subtitles from {encoding}: {err}");
            open(None)
        }
        None => open(None),
    }
    .map_err(|err| Error::Source(format!("can't read the subtitles: {err}")))
}

/// The text of a decoded ASS event: the last of its fields, after the
/// read order, layer, style, name, margins and effect.
fn dialogue_text(line: &str) -> &str {
    let fields = if line.starts_with("Dialogue:") { 10 } else { 9 };
    line.splitn(fields, ',').nth(fields - 1).unwrap_or(line)
}

/// The style of a decoded ASS event: after its read order and layer, or
/// a whole dialogue line's layer, start and end.
fn dialogue_style(line: &str) -> &str {
    let index = if line.starts_with("Dialogue:") { 3 } else { 2 };
    line.split(',').nth(index).map_or("", str::trim)
}

/// ASS dialogue text as spans: override blocks (`{\i1}`) set the style,
/// `\N` breaks lines, `\h` is a hard space; drawings are skipped.
fn from_ass(source: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut style = Span::default();
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
                let on = || value.trim().parse::<i32>() != Ok(0);
                match name {
                    "i" => style.italic = on(),
                    "b" => style.bold = on(),
                    "p" => drawing = on(),
                    _ if name.starts_with('r') => {
                        style.italic = false;
                        style.bold = false;
                    }
                    _ => {}
                }
            }
            rest = &rest[close + 1..];
            continue;
        }
        if c == '\\' {
            let text = match rest[1..].chars().next() {
                Some('N' | 'n') => Some("\n"),
                Some('h') => Some("\u{a0}"),
                _ => None,
            };
            if let Some(text) = text {
                if !drawing {
                    push(&mut spans, text, style.clone());
                }
                rest = &rest[2..];
                continue;
            }
        }
        if !drawing {
            push(&mut spans, &rest[..c.len_utf8()], style.clone());
        }
        rest = &rest[c.len_utf8()..];
    }
    spans
}

/// Add text in `style`, to the last span when it is in the same one.
fn push(spans: &mut Vec<Span>, text: &str, style: Span) {
    match spans.last_mut() {
        Some(last) if last.italic == style.italic && last.bold == style.bold => {
            last.text.push_str(text);
        }
        _ => spans.push(Span {
            text: text.to_owned(),
            ..style
        }),
    }
}

/// Spans without blank lines (which end a cue in both formats) or space at
/// either end.
fn tidy(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for span in spans {
        let text = span.text.replace("\r\n", "\n").replace('\r', "\n");
        push(
            &mut out,
            &text,
            Span {
                text: String::new(),
                ..span
            },
        );
    }
    for span in &mut out {
        while span.text.contains("\n\n") {
            span.text = span.text.replace("\n\n", "\n");
        }
        span.text = span
            .text
            .split('\n')
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
    }
    // A blank line across two spans.
    let mut newline = false;
    for span in &mut out {
        if newline {
            span.text = span.text.trim_start_matches('\n').to_owned();
        }
        if !span.text.is_empty() {
            newline = span.text.ends_with('\n');
        }
    }
    out.retain(|span| !span.text.is_empty());
    if let Some(first) = out.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    if let Some(last) = out.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    out.retain(|span| !span.text.is_empty());
    out
}

/// `H:MM:SS.mmm`, with `separator` before the milliseconds.
fn timestamp(seconds: f64, separator: char) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The cue's text, styled with `<i>` and `<b>`; escaped for WebVTT.
fn markup(cue: &Cue, vtt: bool) -> String {
    let mut out = String::new();
    for span in &cue.spans {
        let text = if vtt {
            escape(&span.text)
        } else {
            span.text.clone()
        };
        let (open, close) = match (span.italic, span.bold) {
            (true, true) => ("<b><i>", "</i></b>"),
            (true, false) => ("<i>", "</i>"),
            (false, true) => ("<b>", "</b>"),
            (false, false) => ("", ""),
        };
        // Tags don't run across lines in every player.
        let lines: Vec<String> = text
            .split('\n')
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("{open}{line}{close}")
                }
            })
            .collect();
        out.push_str(&lines.join("\n"));
    }
    out
}

/// WebVTT of the cues showing between `from` and `to` (all of them with
/// `None`). For HLS, `mpegts` maps the cues' zero to the media's
/// timestamps, in 90 kHz.
pub(crate) fn webvtt(cues: &[Cue], range: Option<(f64, f64)>, mpegts: Option<u64>) -> String {
    let mut out = String::from("WEBVTT\n");
    if let Some(mpegts) = mpegts {
        let _ = writeln!(out, "X-TIMESTAMP-MAP=MPEGTS:{mpegts},LOCAL:00:00:00.000");
    }
    out.push('\n');
    for cue in cues {
        if let Some((from, to)) = range
            && (cue.end <= from || cue.start >= to)
        {
            continue;
        }
        let _ = write!(
            out,
            "{} --> {}{}\n{}\n\n",
            timestamp(cue.start, '.'),
            timestamp(cue.end, '.'),
            cue.placement.webvtt(),
            markup(cue, true)
        );
    }
    out
}

pub(crate) fn srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (number, cue) in cues.iter().enumerate() {
        let _ = write!(
            out,
            "{}\n{} --> {}\n{}\n\n",
            number + 1,
            timestamp(cue.start, ','),
            timestamp(cue.end, ','),
            markup(cue, false)
        );
    }
    out
}

#[cfg(test)]
#[path = "tests/subtitles.rs"]
mod tests;
