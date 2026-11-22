//! The tracks of a file that can be picked from: its video, audio and
//! subtitle streams and the subtitle files beside it, and how to name them
//! in a menu.

use std::ffi::CString;
use std::path::PathBuf;

use ffmpeg::format::stream::{Disposition, Stream};
use ffmpeg::{ffi, media};
use ffmpeg_next as ffmpeg;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// The stream's index in the file. Subtitle files beside it are
    /// numbered after the file's streams.
    pub index: usize,
    pub kind: TrackKind,
    /// As the file has it, usually an ISO 639-2 code such as `eng`.
    pub language: Option<String>,
    pub title: Option<String>,
    /// FFmpeg's name for the codec, such as `eac3` or `subrip`.
    pub codec: String,
    /// The video's size, or how many audio channels.
    pub format: Option<String>,
    pub default: bool,
    pub forced: bool,
    pub hearing_impaired: bool,
    /// The subtitle file, for subtitles beside the video rather than a
    /// stream in it.
    pub file: Option<PathBuf>,
}

impl Track {
    /// The stream as a track, if it is one that can be played: not cover
    /// art, and with a decoder for it.
    pub fn of(stream: &Stream) -> Option<Self> {
        let parameters = stream.parameters();
        let disposition = stream.disposition();
        let kind = match parameters.medium() {
            media::Type::Video if !disposition.contains(Disposition::ATTACHED_PIC) => {
                TrackKind::Video
            }
            media::Type::Audio => TrackKind::Audio,
            media::Type::Subtitle => TrackKind::Subtitle,
            _ => return None,
        };
        ffmpeg::decoder::find(parameters.id())?;
        let metadata = stream.metadata();
        let tag = |key: &str| {
            metadata
                .get(key)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        // SAFETY: the parameters belong to the stream, which outlives this call.
        let raw = unsafe { &*parameters.as_ptr() };
        let format = match kind {
            TrackKind::Video if raw.width > 0 && raw.height > 0 => {
                Some(format!("{}×{}", raw.width, raw.height))
            }
            TrackKind::Audio => channels_name(raw.ch_layout.nb_channels),
            _ => None,
        };
        Some(Self {
            index: stream.index(),
            kind,
            language: tag("language").filter(|code| !matches!(code.as_str(), "und" | "zxx")),
            title: tag("title"),
            codec: parameters.id().name().to_owned(),
            format,
            default: disposition.contains(Disposition::DEFAULT),
            forced: disposition.contains(Disposition::FORCED),
            hearing_impaired: disposition.contains(Disposition::HEARING_IMPAIRED),
            file: None,
        })
    }

    /// What the menu calls the track: its language and title, or its
    /// `number` among the tracks of its kind (from 1) when it has neither.
    pub fn label(&self, number: usize) -> String {
        let language = self.language.as_deref().map(language_name);
        let mut label = match (&language, &self.title) {
            (Some(language), Some(title)) if !title.eq_ignore_ascii_case(language) => {
                format!("{language} – {title}")
            }
            (Some(language), _) => language.clone(),
            (None, Some(title)) => title.clone(),
            (None, None) => format!("Track {number}"),
        };
        let named = label.to_lowercase();
        if self.forced && !named.contains("forced") {
            label.push_str(" (Forced)");
        }
        if self.hearing_impaired && !named.contains("sdh") && !named.contains("hearing") {
            label.push_str(" (SDH)");
        }
        label
    }

    /// Subtitles that are text rather than pictures.
    pub fn is_text(&self) -> bool {
        let Ok(name) = CString::new(self.codec.as_str()) else {
            return false;
        };
        // SAFETY: looks the codec up in FFmpeg's table of them, which
        // lives as long as the program.
        unsafe {
            let descriptor = ffi::avcodec_descriptor_get_by_name(name.as_ptr());
            !descriptor.is_null() && (*descriptor).props & ffi::AV_CODEC_PROP_TEXT_SUB != 0
        }
    }

    /// The codec and format, such as "E-AC-3 · 5.1", and "External" for
    /// a subtitle file.
    pub fn detail(&self) -> String {
        let mut parts = vec![codec_display_name(&self.codec)];
        parts.extend(self.format.clone());
        if self.file.is_some() {
            parts.push("External".to_owned());
        }
        parts.join(" · ")
    }
}

fn channels_name(channels: i32) -> Option<String> {
    Some(match channels {
        ..=0 => return None,
        1 => "Mono".to_owned(),
        2 => "Stereo".to_owned(),
        6 => "5.1".to_owned(),
        8 => "7.1".to_owned(),
        n => format!("{n} channels"),
    })
}

fn codec_display_name(codec: &str) -> String {
    let name = match codec {
        "h264" => "H.264",
        "hevc" => "HEVC",
        "av1" => "AV1",
        "vp8" => "VP8",
        "vp9" => "VP9",
        "mpeg2video" => "MPEG-2",
        "mpeg4" => "MPEG-4",
        "prores" => "ProRes",
        "aac" => "AAC",
        "ac3" => "AC-3",
        "eac3" => "E-AC-3",
        "truehd" => "TrueHD",
        "mlp" => "MLP",
        "dts" => "DTS",
        "flac" => "FLAC",
        "alac" => "ALAC",
        "opus" => "Opus",
        "vorbis" => "Vorbis",
        "mp2" => "MP2",
        "mp3" => "MP3",
        "subrip" | "srt" => "SRT",
        "ass" | "ssa" => "ASS",
        "webvtt" => "WebVTT",
        "mov_text" => "Timed Text",
        "text" => "Text",
        "hdmv_pgs_subtitle" => "PGS",
        "dvd_subtitle" => "VobSub",
        "dvb_subtitle" => "DVB",
        "eia_608" => "CEA-608",
        other if other.starts_with("pcm_") => "PCM",
        other => return other.to_uppercase(),
    };
    name.to_owned()
}

/// Language codes (ISO 639-2, both forms, and 639-1) and their English names.
const LANGUAGES: &[(&[&str], &str)] = &[
    (&["eng", "en"], "English"),
    (&["fre", "fra", "fr"], "French"),
    (&["ger", "deu", "de"], "German"),
    (&["spa", "es"], "Spanish"),
    (&["ita", "it"], "Italian"),
    (&["por", "pt"], "Portuguese"),
    (&["rus", "ru"], "Russian"),
    (&["jpn", "ja"], "Japanese"),
    (&["chi", "zho", "zh"], "Chinese"),
    (&["cmn"], "Mandarin"),
    (&["yue"], "Cantonese"),
    (&["kor", "ko"], "Korean"),
    (&["ara", "ar"], "Arabic"),
    (&["heb", "he"], "Hebrew"),
    (&["hin", "hi"], "Hindi"),
    (&["dut", "nld", "nl"], "Dutch"),
    (&["swe", "sv"], "Swedish"),
    (&["nor", "no"], "Norwegian"),
    (&["nob", "nb"], "Norwegian Bokmål"),
    (&["nno", "nn"], "Norwegian Nynorsk"),
    (&["dan", "da"], "Danish"),
    (&["fin", "fi"], "Finnish"),
    (&["ice", "isl", "is"], "Icelandic"),
    (&["pol", "pl"], "Polish"),
    (&["cze", "ces", "cs"], "Czech"),
    (&["slo", "slk", "sk"], "Slovak"),
    (&["hun", "hu"], "Hungarian"),
    (&["rum", "ron", "ro"], "Romanian"),
    (&["gre", "ell", "el"], "Greek"),
    (&["tur", "tr"], "Turkish"),
    (&["ukr", "uk"], "Ukrainian"),
    (&["bul", "bg"], "Bulgarian"),
    (&["hrv", "hr"], "Croatian"),
    (&["srp", "sr"], "Serbian"),
    (&["bos", "bs"], "Bosnian"),
    (&["slv", "sl"], "Slovenian"),
    (&["mac", "mkd", "mk"], "Macedonian"),
    (&["alb", "sqi", "sq"], "Albanian"),
    (&["est", "et"], "Estonian"),
    (&["lav", "lv"], "Latvian"),
    (&["lit", "lt"], "Lithuanian"),
    (&["tha", "th"], "Thai"),
    (&["vie", "vi"], "Vietnamese"),
    (&["ind", "id"], "Indonesian"),
    (&["may", "msa", "ms"], "Malay"),
    (&["fil"], "Filipino"),
    (&["tgl", "tl"], "Tagalog"),
    (&["per", "fas", "fa"], "Persian"),
    (&["urd", "ur"], "Urdu"),
    (&["ben", "bn"], "Bengali"),
    (&["tam", "ta"], "Tamil"),
    (&["tel", "te"], "Telugu"),
    (&["mar", "mr"], "Marathi"),
    (&["kan", "kn"], "Kannada"),
    (&["mal", "ml"], "Malayalam"),
    (&["pan", "pa"], "Punjabi"),
    (&["guj", "gu"], "Gujarati"),
    (&["cat", "ca"], "Catalan"),
    (&["baq", "eus", "eu"], "Basque"),
    (&["glg", "gl"], "Galician"),
    (&["wel", "cym", "cy"], "Welsh"),
    (&["gle", "ga"], "Irish"),
    (&["lat", "la"], "Latin"),
    (&["afr", "af"], "Afrikaans"),
    (&["swa", "sw"], "Swahili"),
    (&["geo", "kat", "ka"], "Georgian"),
    (&["arm", "hye", "hy"], "Armenian"),
    (&["aze", "az"], "Azerbaijani"),
    (&["kaz", "kk"], "Kazakh"),
    (&["mul"], "Multiple languages"),
];

/// The English name of a language code (ISO 639-2, either form, or 639-1),
/// with any region after it, as in "Portuguese (BR)" for `pt-BR`. Codes not
/// in the table are shown as they are.
pub fn language_name(code: &str) -> String {
    let name = |code: &str| {
        let lower = code.to_ascii_lowercase();
        LANGUAGES
            .iter()
            .find(|(codes, _)| codes.contains(&lower.as_str()))
            .map(|(_, name)| *name)
    };
    if let Some(name) = name(code) {
        return name.to_owned();
    }
    match code.split_once(['-', '_']) {
        Some((base, region)) if !region.is_empty() => match name(base) {
            Some(name) => format!("{name} ({region})"),
            None => code.to_owned(),
        },
        _ => code.to_owned(),
    }
}

/// A language code as BCP 47 has it, as the web and Cast take it: ISO
/// 639-1's two letters where the language has them, else as it is, with
/// any region after it.
pub fn language_tag(code: &str) -> String {
    let (base, region) = match code.split_once(['-', '_']) {
        Some((base, region)) if !region.is_empty() => (base, Some(region)),
        _ => (code, None),
    };
    let lower = base.to_ascii_lowercase();
    let base = LANGUAGES
        .iter()
        .find(|(codes, _)| codes.contains(&lower.as_str()))
        .and_then(|(codes, _)| codes.iter().find(|code| code.len() == 2))
        .map_or(lower.as_str(), |code| code);
    match region {
        Some(region) => format!("{base}-{region}"),
        None => base.to_owned(),
    }
}

/// The language a word names, as a code: the word itself if it is a code
/// in the table, or the code for an English name such as "english".
pub fn language_code(word: &str) -> Option<String> {
    let lower = word.to_ascii_lowercase();
    LANGUAGES.iter().find_map(|(codes, name)| {
        if codes.contains(&lower.as_str()) {
            Some(lower.clone())
        } else if name.eq_ignore_ascii_case(word) {
            Some(codes[0].to_owned())
        } else {
            None
        }
    })
}

/// The language a word names, by a code in the table or its English name,
/// as the table's first code for it: `eng` for `en`, `eng` or "English".
pub fn language(word: &str) -> Option<&'static str> {
    let lower = word.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|(codes, name)| codes.contains(&lower.as_str()) || name.eq_ignore_ascii_case(word))
        .map(|(codes, _)| codes[0])
}

/// Every language a file's tracks can be preferred in, by its first code
/// and its English name, A to Z: those in the table, but "multiple".
pub fn languages() -> Vec<(&'static str, &'static str)> {
    let mut all: Vec<_> = LANGUAGES
        .iter()
        .filter(|(codes, _)| codes[0] != "mul")
        .map(|(codes, name)| (codes[0], *name))
        .collect();
    all.sort_by_key(|(_, name)| *name);
    all
}

/// Whether two language codes name the same language, whichever form each
/// is in, and with any region aside: `eng`, `en` and `en-GB` do.
pub fn same_language(a: &str, b: &str) -> bool {
    let base = |code: &str| {
        let tag = language_tag(code);
        tag.split('-').next().unwrap_or_default().to_owned()
    };
    base(a) == base(b)
}

#[cfg(test)]
#[path = "tests/tracks.rs"]
mod tests;
