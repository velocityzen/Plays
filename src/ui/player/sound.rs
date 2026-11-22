//! What a block says of a sound file under its cover: its title, who made
//! it, its album, and its quality.

use gpui::SharedString;

use super::console::{bit_rate, joined, sample_rate, with_profile};
use crate::media::decoder::MediaInfo;

/// The lines under a sound file's cover, each there only if the file says.
#[derive(Debug, PartialEq)]
pub(super) struct SoundLines {
    pub title: SharedString,
    /// The artist, or the album artist if it has none.
    pub artist: Option<String>,
    /// The album, and its artist if that isn't the one above.
    pub album: Option<String>,
    pub quality: Option<String>,
}

impl SoundLines {
    /// The file's lines, titled `name` if its tags have no title.
    pub(super) fn of(info: &MediaInfo, name: &SharedString) -> Self {
        let other_album_artist = info.album_artist.clone().filter(|album_artist| {
            info.artist
                .as_deref()
                .is_some_and(|artist| !same(artist, album_artist))
        });
        let album = match (info.album.clone(), other_album_artist) {
            (Some(album), Some(by)) => Some(format!("{album} — {by}")),
            (album, by) => album.or(by),
        };
        Self {
            title: info
                .title
                .clone()
                .map_or_else(|| name.clone(), SharedString::from),
            artist: info.artist.clone().or_else(|| info.album_artist.clone()),
            album,
            quality: quality(info),
        }
    }
}

fn same(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

/// Such as "AAC LC · 44.1 kHz · Stereo · 290 kb/s", or "FLAC · 24-bit ·
/// 96 kHz · Stereo · 2.3 Mb/s": the sound's own bit rate, or the file's.
fn quality(info: &MediaInfo) -> Option<String> {
    let audio = info.audio_details.as_ref()?;
    let codec = info
        .audio_codec
        .as_deref()
        .map(codec_name)
        .unwrap_or_default();
    let channels = match audio.layout.as_deref() {
        Some(layout) => capitalized(layout),
        None => format!("{} channels", audio.channels),
    };
    Some(joined([
        with_profile(codec, audio.profile.as_deref()),
        audio
            .bits
            .map(|bits| format!("{bits}-bit"))
            .unwrap_or_default(),
        if audio.sample_rate > 0 {
            sample_rate(audio.sample_rate)
        } else {
            String::new()
        },
        channels,
        audio
            .bit_rate
            .or(info.container.bit_rate)
            .map(bit_rate)
            .unwrap_or_default(),
    ]))
}

/// What a codec is commonly called, from FFmpeg's name for it.
pub(super) fn codec_name(codec: &str) -> String {
    let name = match codec {
        "aac" | "aac_latm" => "AAC",
        "mp3" => "MP3",
        "mp2" => "MP2",
        "flac" => "FLAC",
        "alac" => "ALAC",
        "opus" => "Opus",
        "vorbis" => "Vorbis",
        "ac3" => "AC-3",
        "eac3" => "E-AC-3",
        "dts" => "DTS",
        "truehd" => "TrueHD",
        "wavpack" => "WavPack",
        "ape" => "Monkey's Audio",
        "wmav1" | "wmav2" => "WMA",
        "wmapro" => "WMA Pro",
        "wmalossless" => "WMA Lossless",
        pcm if pcm.starts_with("pcm_") => "PCM",
        dsd if dsd.starts_with("dsd_") => "DSD",
        other => return other.to_uppercase(),
    };
    name.to_owned()
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "tests/sound.rs"]
mod tests;
