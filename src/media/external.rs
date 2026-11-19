//! Subtitle files that go with a video: beside it and named after it
//! (`Movie.srt`, `Movie.en.srt`, `Movie.English.forced.ass` for
//! `Movie.mkv`), the same in a `Subs` or `Subtitles` folder beside it, and
//! every subtitle file in `Subs/Movie/`, as releases often lay them out.
//!
//! A file's name says its language and whether it is forced or for the
//! hearing impaired. When one is picked, all its packets are read (subtitle
//! files are small) and fed to its decoder a little ahead of the playhead.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use ffmpeg::{Packet, media};
use ffmpeg_next as ffmpeg;

use super::subtitles::SubtitleDecoder;
use super::tracks::{Track, language_code};

/// The subtitle files FFmpeg reads, by extension.
const EXTENSIONS: &[&str] = &[
    "srt", "ass", "ssa", "vtt", "sup", "idx", "sub", "smi", "sami",
];

/// The ones that are bitmaps rather than text.
const BITMAP_EXTENSIONS: &[&str] = &["sup", "idx"];

/// How much of a text file is read to tell its encoding.
const ENCODING_SAMPLE_BYTES: u64 = 4 * 1024 * 1024;

/// After a seek, packets from this long before it are decoded again, for
/// the cues still showing then.
const LOOKBACK: f64 = 30.0;

/// A subtitle file found for a video.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub naming: Naming,
}

/// What a subtitle file's name says about it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Naming {
    pub language: Option<String>,
    /// The rest of the name, if any.
    pub title: Option<String>,
    pub forced: bool,
    pub hearing_impaired: bool,
    pub default: bool,
}

/// The subtitle files for the video at `video`, those beside it first,
/// each folder's in name order.
pub fn find(video: &Path) -> Vec<Found> {
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem().and_then(|s| s.to_str()))
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    collect(dir, Some(stem), &mut found);
    for folder in folders(dir, |name| {
        name.eq_ignore_ascii_case("subs") || name.eq_ignore_ascii_case("subtitles")
    }) {
        collect(&folder, Some(stem), &mut found);
        for inner in folders(&folder, |name| name.eq_ignore_ascii_case(stem)) {
            collect(&inner, None, &mut found);
        }
    }
    found
}

/// The folders in `dir` whose names pass `matches`.
fn folders(dir: &Path, matches: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(&matches)
        })
        .collect();
    folders.sort();
    folders
}

/// Add the subtitle files in `dir` named after `stem`, or all of them.
fn collect(dir: &Path, stem: Option<&str>, found: &mut Vec<Found>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter_map(|path| Some((path.file_name()?.to_str()?.to_owned(), path)))
        .filter(|(name, _)| split_extension(name).is_some())
        .collect();
    files.sort_by_cached_key(|(name, _)| name.to_lowercase());
    // VobSub is an `.idx` file and a `.sub` file; FFmpeg opens the `.idx`.
    let vobsubs: Vec<String> = files
        .iter()
        .filter_map(|(name, _)| split_extension(name))
        .filter(|(_, extension)| extension == "idx")
        .map(|(base, _)| base.to_lowercase())
        .collect();
    for (name, path) in &files {
        let Some((base, extension)) = split_extension(name) else {
            continue;
        };
        if extension == "sub" && vobsubs.contains(&base.to_lowercase()) {
            continue;
        }
        let middle = match stem {
            None => base,
            Some(stem) => match strip_prefix_ignore_case(base, stem) {
                Some("") => "",
                Some(rest) => match rest.strip_prefix('.') {
                    Some(middle) => middle,
                    // Another video's, such as `Movie 2.srt` for `Movie.mkv`.
                    None => continue,
                },
                None => continue,
            },
        };
        found.push(Found {
            path: path.clone(),
            naming: Naming::parse(middle),
        });
    }
}

/// A subtitle file's name without its extension, and the extension in
/// lowercase; `None` for other files.
fn split_extension(name: &str) -> Option<(&str, String)> {
    let (base, extension) = name.rsplit_once('.')?;
    let extension = extension.to_ascii_lowercase();
    (!base.is_empty() && EXTENSIONS.contains(&extension.as_str())).then_some((base, extension))
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

impl Naming {
    /// Read the part of a file name between the video's name and the
    /// extension, such as `en.forced` or `2_English`.
    fn parse(middle: &str) -> Self {
        let mut naming = Self::default();
        let mut title = Vec::new();
        for part in middle.split('.').filter(|part| !part.is_empty()) {
            if naming.flag(part) {
                continue;
            }
            if naming.language.is_none()
                && let Some(tag) = language_tag(part)
            {
                naming.language = Some(tag);
                continue;
            }
            // Words, as in `2_English` or `English SDH`. Only names count
            // as languages here: short words are too often codes.
            let mut words = Vec::new();
            for word in part.split(['_', ' ', '-']).filter(|word| !word.is_empty()) {
                if naming.flag(word) {
                    continue;
                }
                if naming.language.is_none()
                    && word.len() > 3
                    && let Some(code) = language_code(word)
                {
                    naming.language = Some(code);
                    continue;
                }
                // Release numbering, as in `2_English`.
                if !word.chars().all(|c| c.is_ascii_digit()) {
                    words.push(word);
                }
            }
            if !words.is_empty() {
                title.push(words.join(" "));
            }
        }
        naming.title = (!title.is_empty()).then(|| title.join(" "));
        naming
    }

    /// Note a word that flags the subtitles, saying whether it was one.
    fn flag(&mut self, word: &str) -> bool {
        match word.to_ascii_lowercase().as_str() {
            "forced" | "foreign" => self.forced = true,
            "sdh" | "cc" | "hoh" => self.hearing_impaired = true,
            // Hindi, unless a language came first, as in `en.hi`.
            "hi" if self.language.is_some() => self.hearing_impaired = true,
            "default" => self.default = true,
            _ => return false,
        }
        true
    }
}

/// A language code or English name, or a code with a region, as in `pt-BR`.
fn language_tag(part: &str) -> Option<String> {
    if let Some(code) = language_code(part) {
        return Some(code);
    }
    let (base, region) = part.split_once(['-', '_'])?;
    let base = language_code(base).filter(|_| base.len() <= 3)?;
    ((2..=4).contains(&region.len()) && region.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| format!("{base}-{region}"))
}

impl Found {
    /// The file as a subtitle track numbered `index`, if FFmpeg can read it.
    pub fn track(&self, index: usize) -> Result<Track> {
        let input = ffmpeg::format::input(&self.path).context("FFmpeg can't read it")?;
        let stream = input
            .streams()
            .best(media::Type::Subtitle)
            .context("it has no subtitles")?;
        let mut track = Track::of(&stream).context("there is no decoder for its subtitles")?;
        let naming = &self.naming;
        track.index = index;
        track.file = Some(self.path.clone());
        // VobSub says its language; text files only have their names.
        track.language = track.language.or_else(|| naming.language.clone());
        track.title = naming.title.clone().or(track.title);
        track.forced |= naming.forced;
        track.hearing_impaired |= naming.hearing_impaired;
        track.default = naming.default;
        if track.language.is_none() && track.title.is_none() {
            track.title = self
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
        }
        Ok(track)
    }
}

/// A subtitle file being played: all its packets, in time order, and the
/// next one to decode.
pub struct SubtitleFile {
    packets: Vec<Packet>,
    time_base: f64,
    next: usize,
}

impl SubtitleFile {
    /// Read the subtitle file at `path`, numbered `index` among the tracks,
    /// and open a decoder for it. Bitmaps that don't say what they are
    /// placed on go on `canvas`.
    pub fn open(
        path: &Path,
        index: usize,
        canvas: Option<(u32, u32)>,
    ) -> Result<(SubtitleDecoder, Self)> {
        let mut input = ffmpeg::format::input(path).context("FFmpeg can't read it")?;
        let stream = input
            .streams()
            .best(media::Type::Subtitle)
            .context("it has no subtitles")?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let parameters = stream.parameters();
        let encoding = text_encoding(path);
        if let Some(encoding) = encoding {
            log::info!("{}: reading it as {encoding}", path.display());
        }
        let decoder = SubtitleDecoder::new(index, &parameters, time_base, canvas, encoding)?;
        let mut packets: Vec<Packet> = input
            .packets()
            .filter(|(stream, _)| stream.index() == stream_index)
            .map(|(_, packet)| packet)
            .collect();
        packets.sort_by_key(|packet| packet.pts().or(packet.dts()).unwrap_or(i64::MIN));
        Ok((
            decoder,
            Self {
                packets,
                time_base: f64::from(time_base),
                next: 0,
            },
        ))
    }

    fn time(&self, packet: &Packet) -> f64 {
        packet
            .pts()
            .or(packet.dts())
            .map_or(f64::NEG_INFINITY, |time| time as f64 * self.time_base)
    }

    /// Go back to a little before `time`.
    pub fn seek(&mut self, time: f64) {
        self.next = self
            .packets
            .partition_point(|packet| self.time(packet) < time - LOOKBACK);
    }

    /// The next packet, if it is due by `until`.
    pub fn next(&mut self, until: f64) -> Option<&Packet> {
        let packet = self.packets.get(self.next)?;
        if self.time(packet) > until {
            return None;
        }
        self.next += 1;
        Some(packet)
    }
}

/// The encoding of a text subtitle file that is neither UTF-8 nor UTF-16
/// (which FFmpeg reads itself), by its iconv name, for FFmpeg to convert.
/// Reads some of it, so not for the UI thread.
pub fn text_encoding(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    let (_, extension) = split_extension(name)?;
    if BITMAP_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(ENCODING_SAMPLE_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    if encoding_rs::Encoding::for_bom(&bytes).is_some() {
        return None;
    }
    match std::str::from_utf8(&bytes) {
        Ok(_) => return None,
        // Cut off in the middle of a character by the sample's size.
        Err(err) if err.error_len().is_none() => return None,
        Err(_) => {}
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(&bytes, true);
    let encoding = detector.guess(None, false);
    // WHATWG's names, which iconv knows but for these two.
    Some(if encoding == encoding_rs::X_MAC_CYRILLIC {
        "MACCYRILLIC"
    } else if encoding == encoding_rs::ISO_8859_8_I {
        "ISO-8859-8"
    } else {
        encoding.name()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naming(middle: &str) -> Naming {
        Naming::parse(middle)
    }

    #[test]
    fn reads_names() {
        assert_eq!(naming(""), Naming::default());
        let english = Naming {
            language: Some("en".into()),
            ..Naming::default()
        };
        assert_eq!(naming("en"), english);
        assert_eq!(naming("EN"), english);
        assert_eq!(
            naming("eng.forced"),
            Naming {
                language: Some("eng".into()),
                forced: true,
                ..Naming::default()
            }
        );
        assert_eq!(
            naming("English.SDH"),
            Naming {
                language: Some("eng".into()),
                hearing_impaired: true,
                ..Naming::default()
            }
        );
        assert!(naming("en.hi").hearing_impaired);
        assert_eq!(naming("hi").language.as_deref(), Some("hi"));
        assert_eq!(naming("pt-BR").language.as_deref(), Some("pt-BR"));
        assert_eq!(
            naming("2_English"),
            Naming {
                language: Some("eng".into()),
                ..Naming::default()
            }
        );
        assert_eq!(
            naming("en.Director's_Cut"),
            Naming {
                language: Some("en".into()),
                title: Some("Director's Cut".into()),
                ..Naming::default()
            }
        );
        assert_eq!(naming("Commentary").title.as_deref(), Some("Commentary"));
    }

    #[test]
    fn finds_files_for_a_video() {
        let dir = std::env::temp_dir()
            .join("plays-tests")
            .join("find-external");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Subs").join("movie")).unwrap();
        for name in [
            "Movie.mkv",
            "Movie.srt",
            "movie.en.forced.ASS",
            "Movie 2.srt",
            "Movie.nfo",
            "Other.srt",
            "Movie.de.idx",
            "Movie.de.sub",
            "Movie.fr.sub",
            "Subs/Movie.es.vtt",
            "Subs/movie/2_English.srt",
        ] {
            fs::write(dir.join(name), "").unwrap();
        }
        let found: Vec<(String, Naming)> = find(&dir.join("Movie.mkv"))
            .into_iter()
            .map(|found| {
                let path = found.path.strip_prefix(&dir).unwrap();
                (path.to_string_lossy().into_owned(), found.naming)
            })
            .collect();
        let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Movie.de.idx",
                "movie.en.forced.ASS",
                "Movie.fr.sub",
                "Movie.srt",
                "Subs/Movie.es.vtt",
                "Subs/movie/2_English.srt",
            ]
        );
        assert!(found[1].1.forced);
        assert_eq!(found[3].1, Naming::default());
        assert_eq!(found[5].1.language.as_deref(), Some("eng"));
    }

    #[test]
    fn tells_text_encodings() {
        let dir = std::env::temp_dir().join("plays-tests").join("encodings");
        fs::create_dir_all(&dir).unwrap();
        let utf8 = dir.join("utf8.srt");
        fs::write(&utf8, "1\n00:00:01,000 --> 00:00:02,000\nПривет, мир\n").unwrap();
        assert_eq!(text_encoding(&utf8), None);
        let (cp1251, _, _) = encoding_rs::WINDOWS_1251
            .encode("1\n00:00:01,000 --> 00:00:02,000\nПривет, как дела? Это субтитры.\n");
        let path = dir.join("cp1251.srt");
        fs::write(&path, &cp1251).unwrap();
        assert_eq!(text_encoding(&path), Some("windows-1251"));
        let bitmap = dir.join("disc.sup");
        fs::write(&bitmap, [0xff, 0xfe, 0x80]).unwrap();
        assert_eq!(text_encoding(&bitmap), None);
    }
}
