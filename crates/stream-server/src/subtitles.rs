//! Subtitles for a cast: every cue of a text track in the media, or of a
//! subtitle file, read once and written as WebVTT (for HLS and Chromecast)
//! or SRT (for DLNA TVs). Italic and bold are kept; placement isn't.
//! Picture subtitles (PGS, DVD) have no text: they go drawn into the
//! video, where it is re-encoded (see `overlay`).

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
            ffmpeg::format::input(path)
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
        let mut spans = Vec::new();
        for rect in subtitle.rects() {
            let more = match rect {
                Rect::Ass(ass) => from_ass(dialogue_text(ass.get())),
                Rect::Text(text) => vec![Span {
                    text: text.get().trim().to_owned(),
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
        let spans = tidy(spans);
        if !spans.is_empty() && end > start {
            cues.push(Cue { start, end, spans });
        }
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(cues)
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
            "{} --> {}\n{}\n\n",
            timestamp(cue.start, '.'),
            timestamp(cue.end, '.'),
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
mod tests {
    use super::*;

    fn span(text: &str, italic: bool, bold: bool) -> Span {
        Span {
            text: text.into(),
            italic,
            bold,
        }
    }

    #[test]
    fn reads_ass_styles_and_breaks() {
        assert_eq!(
            from_ass(r"{\i1}Hello{\i0}\Nworld\hagain"),
            vec![
                span("Hello", true, false),
                span("\nworld\u{a0}again", false, false)
            ]
        );
        assert_eq!(
            from_ass(r"{\b1\i1}both{\r}plain"),
            vec![span("both", true, true), span("plain", false, false)]
        );
        assert_eq!(
            from_ass(r"{\p1}m 0 0 l 10 10{\p0}text"),
            vec![span("text", false, false)]
        );
        assert_eq!(dialogue_text("0,0,Default,,0,0,0,,Hi, there"), "Hi, there");
    }

    #[test]
    fn writes_webvtt_and_srt() {
        let cues = vec![
            Cue {
                start: 0.2,
                end: 1.0,
                spans: vec![span("A & B", false, false), span("\nleaning", true, false)],
            },
            Cue {
                start: 3661.5,
                end: 3662.0,
                spans: vec![span("late", false, false)],
            },
        ];
        assert_eq!(
            webvtt(&cues, None, Some(900_000)),
            "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:900000,LOCAL:00:00:00.000\n\n\
             00:00:00.200 --> 00:00:01.000\nA &amp; B\n<i>leaning</i>\n\n\
             01:01:01.500 --> 01:01:02.000\nlate\n\n"
        );
        assert_eq!(webvtt(&cues, Some((1.0, 2.0)), None), "WEBVTT\n\n");
        assert_eq!(
            srt(&cues),
            "1\n00:00:00,200 --> 00:00:01,000\nA & B\n<i>leaning</i>\n\n\
             2\n01:01:01,500 --> 01:01:02,000\nlate\n\n"
        );
    }

    #[test]
    fn tidies_blank_lines_and_ends() {
        assert_eq!(
            tidy(vec![
                span(" one\r\n\r\n", false, false),
                span("two \n", true, false)
            ]),
            vec![span("one\n", false, false), span("two", true, false)]
        );
    }
}
