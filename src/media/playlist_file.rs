//! Playlist files, `.m3u`, `.m3u8` and `.pls`: what they list, to play one
//! after another in a block. An HLS playlist is a stream, which FFmpeg
//! plays, not a list of files.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use url::Url;

use super::decoder;
use super::source::{Source, media_in};

/// The extensions of playlist files, lowercase.
pub const EXTENSIONS: [&str; 3] = ["m3u", "m3u8", "pls"];

/// A playlist longer than this isn't read.
const MAX_BYTES: usize = 4 << 20;

/// How deep playlists listing playlists are followed.
const MAX_DEPTH: usize = 4;

/// Whether `path` is named as a playlist file is.
pub fn is_playlist_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// Whether `url` names a playlist of files over HTTP, which `fetch` reads:
/// one ending `.m3u` or `.pls`. One ending `.m3u8` is taken for HLS, as
/// nearly all are, and opened as a stream without being read first.
pub fn is_playlist_url(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    let path = url.path().to_ascii_lowercase();
    matches!(url.scheme(), "http" | "https") && (path.ends_with(".m3u") || path.ends_with(".pls"))
}

/// What the playlist file at `path` lists, in order: files and folders,
/// made whole against the folder it's in, and streams. The playlists it
/// lists are read in turn, and the folders listed for their media files.
/// `None` if it can't be read, or is an HLS playlist, a stream to be
/// opened as it is.
///
/// Reads the disk, so not for the UI thread.
pub fn read(path: &Path) -> Option<Vec<Source>> {
    read_at(path, 0)
}

/// What the playlist at `url` lists, as `read` says, its relative
/// addresses made whole against where it came from. Waits on the network,
/// so not for the UI thread.
pub fn fetch(url: &str) -> Option<Vec<Source>> {
    fetch_at(url, 0)
}

fn read_at(path: &Path, depth: usize) -> Option<Vec<Source>> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_BYTES {
        log::warn!("{} is too long to read as a playlist", path.display());
        return None;
    }
    let entries = entries(&text(&bytes))?;
    let folder = path.parent().unwrap_or(Path::new(""));
    let base = Base::Folder(folder);
    Some(listed(&entries, &base, depth))
}

fn fetch_at(url: &str, depth: usize) -> Option<Vec<Source>> {
    let fetched = match decoder::fetch_small(url, MAX_BYTES) {
        Ok(Some(fetched)) => fetched,
        Ok(None) => {
            log::warn!("{url} is too long to read as a playlist");
            return None;
        }
        Err(err) => {
            log::warn!("could not read the playlist: {err:#}");
            return None;
        }
    };
    let entries = entries(&text(&fetched.body))?;
    let base = Url::parse(&fetched.location)
        .or_else(|_| Url::parse(url))
        .ok()?;
    Some(listed(&entries, &Base::Url(&base), depth))
}

/// What `entries` name from `base`, playlists among them read and folders
/// listed, while `depth` allows.
fn listed(entries: &[String], base: &Base, depth: usize) -> Vec<Source> {
    let mut sources = Vec::new();
    for entry in entries {
        let Some(source) = resolve(entry, base) else {
            log::warn!("a playlist lists {entry:?}, which isn't a file or a stream");
            continue;
        };
        let inner = match &source {
            _ if depth >= MAX_DEPTH => None,
            Source::File(path) if path.is_dir() => {
                Some(media_in(path).into_iter().map(Source::File).collect())
            }
            Source::File(path) if is_playlist_file(path) => read_at(path, depth + 1),
            Source::Url(url) if is_playlist_url(url) => fetch_at(url, depth + 1),
            _ => None,
        };
        match inner {
            Some(inner) => sources.extend(inner),
            None => sources.push(source),
        }
    }
    sources
}

/// The text of a playlist: as its byte order mark says, else UTF-8, else
/// in the encoding it looks to be in, as older `.m3u` files are.
pub(crate) fn text(bytes: &[u8]) -> String {
    if let Some((encoding, _)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding.decode_with_bom_removal(bytes).0.into_owned();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let encoding = detector.guess(None, false);
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}

/// The entries of a playlist's `text`, in order, each as it's written:
/// `.pls` if its first line says `[playlist]`, else M3U. `None` for an HLS
/// playlist, whose tags start `#EXT-X-`.
pub(crate) fn entries(text: &str) -> Option<Vec<String>> {
    let lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = lines.clone().next().unwrap_or_default();
    if first.eq_ignore_ascii_case("[playlist]") {
        return Some(pls(lines));
    }
    if lines.clone().any(|line| line.starts_with("#EXT-X-")) {
        return None;
    }
    Some(
        lines
            .filter(|line| !line.starts_with('#'))
            .map(str::to_owned)
            .collect(),
    )
}

/// A `.pls` file's `FileN=` entries, by `N`; the same `N` twice, the first.
fn pls<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut files: Vec<(u64, String)> = lines
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            let number = key
                .get(..4)
                .filter(|start| start.eq_ignore_ascii_case("file"))
                .and_then(|_| key[4..].parse().ok())?;
            let value = value.trim();
            (!value.is_empty()).then(|| (number, value.to_owned()))
        })
        .collect();
    files.sort_by_key(|(number, _)| *number);
    files.dedup_by_key(|(number, _)| *number);
    files.into_iter().map(|(_, file)| file).collect()
}

/// Where a playlist is, which its relative entries are relative to.
pub(crate) enum Base<'a> {
    Folder(&'a Path),
    Url(&'a Url),
}

/// What `entry`, from a playlist at `base`, names: a stream, a file by its
/// URL, or a file by its path, whole or from `base`. `None` for an
/// address Plays can't open, or a Windows path anywhere else.
pub(crate) fn resolve(entry: &str, base: &Base) -> Option<Source> {
    let entry = entry.trim();
    if entry.contains("://") {
        if entry
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"))
        {
            let url = Url::parse(entry).ok()?;
            return url.to_file_path().ok().map(Source::File);
        }
        return Source::parse_url(entry).ok();
    }
    match base {
        Base::Folder(folder) => {
            let path = local_path(entry)?;
            Some(Source::File(if path.is_absolute() {
                path
            } else {
                folder.join(path)
            }))
        }
        Base::Url(base) => {
            let joined = base.join(&entry.replace('\\', "/")).ok()?;
            Source::parse_url(joined.as_str()).ok()
        }
    }
}

/// `entry` as a path here. A playlist made on Windows separates folders
/// with backslashes, and may name a drive, which only Windows has.
fn local_path(entry: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        return Some(PathBuf::from(entry));
    }
    let bytes = entry.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive || entry.starts_with("\\\\") {
        return None;
    }
    Some(PathBuf::from(entry.replace('\\', "/")))
}

#[cfg(test)]
#[path = "tests/playlist_file.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/playlist_file_properties.rs"]
mod properties;
