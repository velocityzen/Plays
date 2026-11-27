//! Subtitle files that go with a video: beside it and named after it
//! (`Movie.srt`, `Movie.en.srt`, `Movie.English.forced.ass` for
//! `Movie.mkv`), the same in a `Subs` or `Subtitles` folder beside it, and
//! every subtitle file in `Subs/Movie/`, as releases often lay them out.
//!
//! A file's name says its language and whether it is forced or for the
//! hearing impaired. They are opened, to see what they hold, while the
//! video opens, several at once. When one is picked, all its packets are
//! read, on a thread of its own, as a bitmap file can be tens of megabytes
//! and on a share far away, and fed to its decoder a little ahead of the
//! playhead.

use std::fs::{self, DirEntry, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use anyhow::{Context as _, Result, anyhow, bail};
use ffmpeg::{Packet, media};
use ffmpeg_next as ffmpeg;

use super::source::{is_file, is_folder};
use super::subtitles::SubtitleDecoder;
use super::tracks::{Track, language_code};

/// The subtitle files FFmpeg reads, by extension.
const EXTENSIONS: &[&str] = &[
    "srt", "ass", "ssa", "vtt", "sup", "idx", "sub", "smi", "sami",
];

/// The ones that are bitmaps rather than text.
const BITMAP_EXTENSIONS: &[&str] = &["sup", "idx"];

/// How many subtitle files are opened at once.
const OPENED_AT_ONCE: usize = 8;

/// How much of a text file is read to tell its encoding.
const ENCODING_SAMPLE_BYTES: u64 = 4 * 1024 * 1024;

/// After a seek, packets from this long before it are decoded again, for
/// the cues still showing then.
const LOOKBACK: f64 = 30.0;

/// For tests: files that take this much longer to open (`Found::track`)
/// or read (`SubtitleFile::open`), as on a share far away.
#[cfg(test)]
pub(super) mod slow {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    pub static TO_OPEN: Mutex<Vec<(PathBuf, Duration)>> = Mutex::new(Vec::new());
    pub static TO_READ: Mutex<Vec<(PathBuf, Duration)>> = Mutex::new(Vec::new());

    /// Take as long as `files` say `path` takes.
    pub fn wait(files: &Mutex<Vec<(PathBuf, Duration)>>, path: &Path) {
        let slow = files
            .lock()
            .unwrap()
            .iter()
            .find(|(slow, _)| slow == path)
            .map(|(_, took)| *took);
        if let Some(took) = slow {
            std::thread::sleep(took);
        }
    }
}

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
    let beside = listing(dir);
    collect(&beside, Some(stem), &mut found);
    for folder in folders(&beside, |name| {
        name.eq_ignore_ascii_case("subs") || name.eq_ignore_ascii_case("subtitles")
    }) {
        let in_folder = listing(&folder);
        collect(&in_folder, Some(stem), &mut found);
        for inner in folders(&in_folder, |name| name.eq_ignore_ascii_case(stem)) {
            collect(&listing(&inner), None, &mut found);
        }
    }
    found
}

/// What's in `dir`, read once; nothing if it can't be.
fn listing(dir: &Path) -> Vec<DirEntry> {
    fs::read_dir(dir)
        .map(|entries| entries.flatten().collect())
        .unwrap_or_default()
}

/// The folders among `entries` whose names pass `matches`.
fn folders(entries: &[DirEntry], matches: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = entries
        .iter()
        .filter(|entry| entry.file_name().to_str().is_some_and(&matches))
        .filter(|entry| is_folder(entry))
        .map(DirEntry::path)
        .collect();
    folders.sort();
    folders
}

/// Add the subtitle files among `entries` named after `stem`, or all of
/// them.
fn collect(entries: &[DirEntry], stem: Option<&str>, found: &mut Vec<Found>) {
    let mut files: Vec<(String, PathBuf)> = entries
        .iter()
        .filter_map(|entry| Some((entry.file_name().to_str()?.to_owned(), entry)))
        .filter(|(name, _)| {
            split_extension(name).is_some_and(|(base, _)| middle(base, stem).is_some())
        })
        .filter(|(_, entry)| is_file(entry))
        .map(|(name, entry)| (name, entry.path()))
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
        let Some(middle) = middle(base, stem) else {
            continue;
        };
        found.push(Found {
            path: path.clone(),
            naming: Naming::parse(middle),
        });
    }
}

/// What a subtitle file's name, `base` without its extension, says between
/// the video's name, `stem`, and the extension: `en.forced` of
/// `Movie.en.forced` for `Movie`. All of it if there's no video's name, and
/// `None` for another video's, such as `Movie 2` for `Movie`.
fn middle<'a>(base: &'a str, stem: Option<&str>) -> Option<&'a str> {
    let Some(stem) = stem else {
        return Some(base);
    };
    match strip_prefix_ignore_case(base, stem)? {
        "" => Some(""),
        rest => rest.strip_prefix('.'),
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

/// The subtitle files for the video at `video` that FFmpeg can read, as
/// tracks (numbered 0: they follow the video's), those beside it first.
/// Each is opened to see what it holds, which can take a while, so several
/// at once, each on a thread of its own.
pub fn tracks(video: &Path) -> Vec<Track> {
    let found = find(video);
    let mut tracks = Vec::new();
    for some in found.chunks(OPENED_AT_ONCE) {
        let opened: Vec<Result<Track>> = std::thread::scope(|scope| {
            let opening: Vec<_> = some
                .iter()
                .map(|found| {
                    let thread = std::thread::Builder::new()
                        .name("subtitle file".into())
                        .spawn_scoped(scope, || found.track());
                    (found, thread.ok())
                })
                .collect();
            opening
                .into_iter()
                .map(|(found, thread)| match thread {
                    Some(thread) => thread
                        .join()
                        .unwrap_or_else(|_| Err(anyhow!("opening it panicked"))),
                    // No thread to spare: open it here.
                    None => found.track(),
                })
                .collect()
        });
        for (found, opened) in some.iter().zip(opened) {
            match opened {
                Ok(track) => tracks.push(track),
                Err(err) => log::warn!("{}: skipping it: {err:#}", found.path.display()),
            }
        }
    }
    tracks
}

impl Found {
    /// The file as a subtitle track, numbered 0, if FFmpeg can read it.
    pub fn track(&self) -> Result<Track> {
        #[cfg(test)]
        slow::wait(&slow::TO_OPEN, &self.path);
        let input = ffmpeg_safe::input_file(&self.path).context("FFmpeg can't read it")?;
        let stream = input
            .streams()
            .best(media::Type::Subtitle)
            .context("it has no subtitles")?;
        let mut track = Track::of(&stream).context("there is no decoder for its subtitles")?;
        let naming = &self.naming;
        track.index = 0;
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
    /// and open a decoder for it, unless `stop` is set first. Bitmaps that
    /// don't say what they are placed on go on `canvas`.
    pub fn open(
        path: &Path,
        index: usize,
        canvas: Option<(u32, u32)>,
        stop: &AtomicBool,
    ) -> Result<(SubtitleDecoder, Self)> {
        #[cfg(test)]
        slow::wait(&slow::TO_READ, path);
        let mut input = ffmpeg_safe::input_file(path).context("FFmpeg can't read it")?;
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
        let mut packets: Vec<Packet> = Vec::new();
        for (stream, packet) in input.packets() {
            if stop.load(Ordering::Relaxed) {
                bail!("no longer wanted");
            }
            if stream.index() == stream_index {
                packets.push(packet);
            }
        }
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

/// What reading a subtitle file gives: its decoder and its packets.
type Loaded = Result<(SubtitleDecoder, SubtitleFile)>;

/// A subtitle file being read on a thread of its own, to play once it is.
/// Dropped, it stops reading.
pub struct Reading {
    /// The track it is.
    pub index: usize,
    read: Receiver<Loaded>,
    stop: Arc<AtomicBool>,
}

impl Reading {
    /// Read the file at `path` as track `index` (`SubtitleFile::open`),
    /// calling `done` once it has.
    pub fn start(
        path: PathBuf,
        index: usize,
        canvas: Option<(u32, u32)>,
        done: impl FnOnce() + Send + 'static,
    ) -> Result<Self> {
        let (sender, read) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::Builder::new()
            .name("subtitle file".into())
            .spawn(move || {
                let file = SubtitleFile::open(&path, index, canvas, &stopped)
                    .with_context(|| format!("reading {}", path.display()));
                if sender.send(file).is_ok() {
                    done();
                }
            })
            .context("starting to read it")?;
        Ok(Self { index, read, stop })
    }

    /// Its decoder and packets once it is read, or why it couldn't be.
    pub fn take(&self) -> Option<Loaded> {
        match self.read.try_recv() {
            Ok(read) => Some(read),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(anyhow!("reading it stopped"))),
        }
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
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
    let whole = bytes.len() < ENCODING_SAMPLE_BYTES as usize;
    encoding(&bytes, whole)
}

/// The encoding of `bytes`, a whole file or its start, by its iconv name;
/// none for UTF-8 and UTF-16.
fn encoding(bytes: &[u8], whole: bool) -> Option<&'static str> {
    if encoding_rs::Encoding::for_bom(bytes).is_some() {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => return None,
        // Cut off in the middle of a character by the sample's size.
        Err(err) if !whole && err.error_len().is_none() => return None,
        Err(_) => {}
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
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
#[path = "tests/external.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/external_properties.rs"]
mod properties;
