//! HLS playlists: the master one, naming the stream and its subtitles, and
//! the media ones listing segments; and the RFC 6381 codec names the master
//! playlist gives, read from each stream's parameter sets.

use std::fmt::Write;

use ffmpeg::codec;
use ffmpeg::codec::Id;
use ffmpeg_next as ffmpeg;

use crate::job::LiveWindow;
use crate::profile::{Hdr, Segments};
use crate::segments::Timeline;

/// What the master playlist says about the stream.
pub(crate) struct Variant {
    pub bandwidth: u64,
    /// Every stream's codec, when all of them are known: a partial list
    /// would tell a player there is less in it.
    pub codecs: Option<String>,
    pub resolution: Option<(u32, u32)>,
    pub frame_rate: Option<f64>,
    /// The HDR its video shows, as its `VIDEO-RANGE` says; none for SDR. An
    /// Apple TV plays HDR it's told of alone.
    pub hdr: Option<Hdr>,
}

/// The subtitles the master playlist offers.
pub(crate) struct SubtitlesRendition {
    pub language: Option<String>,
    pub name: String,
}

pub(crate) fn master(variant: &Variant, subtitles: Option<&SubtitlesRendition>) -> String {
    let mut out = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n");
    if let Some(subtitles) = subtitles {
        let _ = write!(
            out,
            "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"{}\",",
            quoted(&subtitles.name)
        );
        if let Some(language) = &subtitles.language {
            let _ = write!(out, "LANGUAGE=\"{}\",", quoted(language));
        }
        out.push_str("DEFAULT=YES,AUTOSELECT=YES,FORCED=NO,URI=\"subs.m3u8\"\n");
    }
    let _ = write!(out, "#EXT-X-STREAM-INF:BANDWIDTH={}", variant.bandwidth);
    if let Some(codecs) = &variant.codecs {
        let _ = write!(out, ",CODECS=\"{codecs}\"");
    }
    if let Some((width, height)) = variant.resolution {
        let _ = write!(out, ",RESOLUTION={width}x{height}");
    }
    if let Some(rate) = variant.frame_rate {
        let _ = write!(out, ",FRAME-RATE={rate:.3}");
    }
    if let Some(hdr) = variant.hdr {
        let _ = write!(out, ",VIDEO-RANGE={}", video_range(hdr));
    }
    if subtitles.is_some() {
        out.push_str(",SUBTITLES=\"subs\"");
    }
    out.push_str(",CLOSED-CAPTIONS=NONE\nvideo.m3u8\n");
    out
}

/// HDR's transfer, as `VIDEO-RANGE` names it: Dolby Vision's is PQ, as
/// its base layer's is when that's HDR10.
fn video_range(hdr: Hdr) -> &'static str {
    match hdr {
        Hdr::Hlg => "HLG",
        Hdr::Hdr10 | Hdr::DolbyVision => "PQ",
    }
}

/// What can't be in a quoted attribute.
fn quoted(text: &str) -> String {
    text.replace(['"', '\n', '\r'], "")
}

pub(crate) fn extension(kind: Segments) -> &'static str {
    match kind {
        Segments::Fmp4 => "m4s",
        Segments::Ts => "ts",
    }
}

fn header(out: &mut String, target: u64, sequence: usize, kind: Option<Segments>) {
    let _ = write!(
        out,
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{target}\n\
         #EXT-X-MEDIA-SEQUENCE:{sequence}\n"
    );
    if kind.is_some() {
        out.push_str("#EXT-X-INDEPENDENT-SEGMENTS\n");
    }
    if kind == Some(Segments::Fmp4) {
        out.push_str("#EXT-X-MAP:URI=\"init.mp4\"\n");
    }
}

/// A file's media playlist, every segment in it.
pub(crate) fn vod(timeline: &Timeline, kind: Segments) -> String {
    let mut out = String::new();
    header(&mut out, timeline.target(), 0, None);
    out.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");
    header_tail(&mut out, kind);
    for segment in 0..timeline.len() {
        let _ = write!(
            out,
            "#EXTINF:{:.3},\n{segment}.{}\n",
            timeline.duration(segment),
            extension(kind)
        );
    }
    out.push_str("#EXT-X-ENDLIST\n");
    out
}

fn header_tail(out: &mut String, kind: Segments) {
    out.push_str("#EXT-X-INDEPENDENT-SEGMENTS\n");
    if kind == Segments::Fmp4 {
        out.push_str("#EXT-X-MAP:URI=\"init.mp4\"\n");
    }
}

/// A live stream's media playlist: the last few segments.
pub(crate) fn live(window: &LiveWindow, kind: Segments) -> String {
    let target = window
        .segments
        .iter()
        .map(|(_, duration)| *duration)
        .fold(1.0_f64, f64::max)
        .ceil() as u64;
    let sequence = window.segments.first().map_or(0, |(sequence, _)| *sequence);
    let mut out = String::new();
    header(&mut out, target, sequence, Some(kind));
    for (sequence, duration) in &window.segments {
        let _ = write!(
            out,
            "#EXTINF:{duration:.3},\n{sequence}.{}\n",
            extension(kind)
        );
    }
    if window.ended {
        out.push_str("#EXT-X-ENDLIST\n");
    }
    out
}

/// The subtitles' playlist: a WebVTT file for each of the video's segments.
pub(crate) fn subtitles(timeline: &Timeline) -> String {
    let mut out = String::new();
    header(&mut out, timeline.target(), 0, None);
    out.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");
    for segment in 0..timeline.len() {
        let _ = write!(
            out,
            "#EXTINF:{:.3},\ns{segment}.vtt\n",
            timeline.duration(segment)
        );
    }
    out.push_str("#EXT-X-ENDLIST\n");
    out
}

/// A video stream's codec name, as `CODECS` takes it.
pub(crate) fn video_codec(parameters: &codec::Parameters) -> Option<String> {
    let extradata = ffmpeg_safe::extradata(parameters);
    match parameters.id() {
        Id::H264 => avc(extradata),
        Id::HEVC => hevc(extradata),
        _ => None,
    }
}

/// An audio stream's codec name, as `CODECS` takes it.
pub(crate) fn audio_codec(parameters: &codec::Parameters) -> Option<String> {
    Some(match parameters.id() {
        Id::AAC => {
            // The object type: from the AudioSpecificConfig, else the
            // profile FFmpeg read (which counts from zero).
            let object = ffmpeg_safe::extradata(parameters)
                .first()
                .map(|byte| byte >> 3)
                .filter(|object| (1..31).contains(object))
                .map(i32::from)
                .unwrap_or_else(|| {
                    // SAFETY: the parameters are valid while borrowed.
                    let profile = unsafe { (*parameters.as_ptr()).profile };
                    if profile >= 0 { profile + 1 } else { 2 }
                });
            format!("mp4a.40.{object}")
        }
        Id::AC3 => "ac-3".into(),
        Id::EAC3 => "ec-3".into(),
        Id::MP3 => "mp4a.40.34".into(),
        Id::OPUS => "Opus".into(),
        Id::FLAC => "fLaC".into(),
        Id::ALAC => "alac".into(),
        _ => return None,
    })
}

/// `avc1.PPCCLL`, from an `avcC` record or an Annex B sequence parameter set.
pub(crate) fn avc(extradata: &[u8]) -> Option<String> {
    let bytes = if extradata.first() == Some(&1) {
        extradata.get(1..4)?
    } else {
        let sps = ffmpeg_safe::nal_units(extradata)
            .find(|nal| nal.first().is_some_and(|b| b & 0x1f == 7))?;
        sps.get(1..4)?
    };
    Some(format!(
        "avc1.{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2]
    ))
}

/// `hvc1.…`, from an `hvcC` record or an Annex B sequence parameter set.
pub(crate) fn hevc(extradata: &[u8]) -> Option<String> {
    let ptl: Vec<u8> = if extradata.first() == Some(&1) {
        extradata.get(1..13)?.to_vec()
    } else {
        let sps = ffmpeg_safe::nal_units(extradata)
            .find(|nal| nal.first().is_some_and(|b| (b >> 1) & 0x3f == 33))?;
        // After the two-byte header and the byte with the parameter set's
        // id and sub-layer count.
        unescape(sps).get(3..15)?.to_vec()
    };
    let space = ["", "A", "B", "C"][usize::from(ptl[0] >> 6)];
    let tier = if ptl[0] & 0x20 != 0 { 'H' } else { 'L' };
    let profile = ptl[0] & 0x1f;
    let compatibility = u32::from_be_bytes([ptl[1], ptl[2], ptl[3], ptl[4]]).reverse_bits();
    let level = ptl[11];
    let mut name = format!("hvc1.{space}{profile}.{compatibility:x}.{tier}{level}");
    let constraints = &ptl[5..11];
    let used = constraints
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |last| last + 1);
    for byte in &constraints[..used] {
        let _ = write!(name, ".{byte:x}");
    }
    Some(name.to_uppercase().replacen("HVC1", "hvc1", 1))
}

/// A NAL unit's payload without its emulation prevention bytes.
fn unescape(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nal.len());
    let mut zeros = 0;
    for &byte in nal {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        out.push(byte);
    }
    out
}

#[cfg(test)]
#[path = "tests/playlist.rs"]
mod tests;
