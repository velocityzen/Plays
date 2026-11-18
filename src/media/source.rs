//! What a player plays: a local file or a network stream.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// URL schemes FFmpeg can stream from.
const STREAM_SCHEMES: &[&str] = &[
    "http", "https", "rtmp", "rtmps", "rtmpe", "rtmpt", "rtmpte", "rtmpts", "rtsp", "rtsps", "rtp",
    "srt", "udp", "tcp", "mms", "mmsh", "mmst", "ftp",
];

/// Extensions of the media files a folder opens to, and that a file's
/// previous and next ones are picked from.
const MEDIA_EXTENSIONS: &[&str] = &[
    "3gp", "aac", "aif", "aiff", "alac", "ape", "avi", "flac", "flv", "gif", "m2ts", "m4a", "m4b",
    "m4v", "mka", "mkv", "mov", "mp2", "mp3", "mp4", "mpeg", "mpg", "mts", "mxf", "oga", "ogg",
    "ogv", "opus", "ts", "vob", "wav", "weba", "webm", "wma", "wmv", "wv",
];

/// The media files directly in `folder`, by name.
///
/// It reads the folder once, and looks no further at entries without a
/// media file's name, nor (usually) at those with one: the listing says
/// what they are, so a folder on a network drive takes one trip, not one a
/// file. Slow there all the same, so not for the UI thread.
pub fn media_in(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| is_media(Path::new(&entry.file_name())))
        .filter(|entry| match entry.file_type() {
            // Links are followed to see what they point at.
            Ok(kind) if kind.is_symlink() => entry.path().is_file(),
            Ok(kind) => kind.is_file(),
            Err(_) => false,
        })
        .map(|entry| entry.path())
        .collect();
    files.sort();
    files
}

/// A file with a media file's extension, and not hidden.
fn is_media(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'));
    let known = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    known && !hidden
}

/// The media files before and after a file in its folder, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Neighbors {
    pub previous: Option<PathBuf>,
    pub next: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    Url(String),
}

impl Source {
    /// Parse a stream URL that was typed or pasted.
    pub fn parse_url(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Enter the address of a stream".into());
        }
        let Some((scheme, rest)) = text.split_once("://") else {
            return Err(
                "That doesn't look like a URL; it should start with https:// or similar".into(),
            );
        };
        let scheme = scheme.to_ascii_lowercase();
        if !STREAM_SCHEMES.contains(&scheme.as_str()) {
            return Err(format!("{scheme}:// streams aren't supported"));
        }
        if rest.is_empty() || rest.starts_with('/') {
            return Err("The URL is missing a host name".into());
        }
        if text.chars().any(char::is_whitespace) {
            return Err("URLs can't contain spaces".into());
        }
        Ok(Self::Url(format!("{scheme}://{rest}")))
    }

    /// A file path, or a URL when given one (for example on the command line).
    pub fn from_arg(arg: PathBuf) -> Self {
        match arg.to_str().map(Self::parse_url) {
            Some(Ok(url)) => url,
            _ => Self::File(arg),
        }
    }

    /// A URL the system asked the app to open: a `file://` URL for a file
    /// or folder opened from Finder or dropped on the Dock icon, or a stream.
    pub fn from_open_url(text: &str) -> Option<Self> {
        match url::Url::parse(text) {
            Ok(url) if url.scheme() == "file" => url.to_file_path().ok().map(Self::File),
            _ => Self::parse_url(text).ok(),
        }
    }

    pub fn is_network(&self) -> bool {
        matches!(self, Self::Url(_))
    }

    /// The media files before and after this file in its folder, by name;
    /// none for a stream. It needn't be a media file itself.
    ///
    /// Lists the folder (`media_in`), so not for the UI thread.
    pub fn neighbors(&self) -> Neighbors {
        let Self::File(path) = self else {
            return Neighbors::default();
        };
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return Neighbors::default();
        };
        // A bare file name is in the current folder.
        let listed = if folder.as_os_str().is_empty() {
            Path::new(".")
        } else {
            folder
        };
        let names: Vec<_> = media_in(listed)
            .into_iter()
            .filter_map(|file| file.file_name().map(ToOwned::to_owned))
            .collect();
        let before = names.partition_point(|other| other.as_os_str() < name);
        let after = names.partition_point(|other| other.as_os_str() <= name);
        Neighbors {
            previous: before.checked_sub(1).map(|i| folder.join(&names[i])),
            next: names.get(after).map(|other| folder.join(other)),
        }
    }

    /// What FFmpeg opens.
    pub(crate) fn location(&self) -> &Path {
        match self {
            Self::File(path) => path,
            Self::Url(url) => Path::new(url),
        }
    }

    /// A short name for the tile: the file name, or the host and last path
    /// segment of a URL.
    pub fn name(&self) -> String {
        match self {
            Self::File(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            Self::Url(url) => {
                let (host, path) = url_parts(url);
                match path.rsplit('/').find(|s| !s.is_empty()) {
                    Some(last) => format!("{host}/{last}"),
                    None => host.to_string(),
                }
            }
        }
    }
}

/// Paths are shown in full. URLs leave out credentials and the query string,
/// which often carries access tokens, so they are safe to log.
impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path) => write!(f, "{}", path.display()),
            Self::Url(url) => {
                let scheme = url.split_once("://").map_or("", |(s, _)| s);
                let (host, path) = url_parts(url);
                write!(f, "{scheme}://{host}{path}")
            }
        }
    }
}

/// The host (with port, without user info) and the path (without query or
/// fragment) of a URL.
fn url_parts(url: &str) -> (&str, &str) {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    (host, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stream_urls() {
        assert_eq!(
            Source::parse_url("  HTTPS://example.com/live/index.m3u8 "),
            Ok(Source::Url("https://example.com/live/index.m3u8".into()))
        );
        assert!(Source::parse_url("rtsp://10.0.0.2:554/stream1").is_ok());
        assert!(Source::parse_url("").is_err());
        assert!(Source::parse_url("example.com/video.mp4").is_err());
        assert!(Source::parse_url("file:///etc/passwd").is_err());
        assert!(Source::parse_url("https://").is_err());
        assert!(Source::parse_url("https://example.com/a b").is_err());
    }

    #[test]
    fn treats_arguments_as_files_unless_they_are_urls() {
        assert_eq!(
            Source::from_arg("movie.mkv".into()),
            Source::File("movie.mkv".into())
        );
        assert!(Source::from_arg("rtmp://example.com/app/key".into()).is_network());
    }

    #[test]
    fn opens_file_urls_from_the_system() {
        assert_eq!(
            Source::from_open_url("file:///Users/me/My%20Movies/clip%231.mkv"),
            Some(Source::File("/Users/me/My Movies/clip#1.mkv".into()))
        );
        assert_eq!(
            Source::from_open_url("file://localhost/tmp/song.mp3"),
            Some(Source::File("/tmp/song.mp3".into()))
        );
        assert!(
            Source::from_open_url("https://example.com/live.m3u8").is_some_and(|s| s.is_network())
        );
        assert_eq!(Source::from_open_url("mailto:me@example.com"), None);
    }

    #[test]
    fn finds_the_media_files_before_and_after_a_file() {
        let folder = std::env::temp_dir().join(format!("plays-neighbors-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["a.mkv", "b.txt", "c.mp3", ".d.mp4", "e.MP4"] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        let neighbors = |name: &str| Source::File(folder.join(name)).neighbors();
        assert_eq!(
            neighbors("c.mp3"),
            Neighbors {
                previous: Some(folder.join("a.mkv")),
                next: Some(folder.join("e.MP4")),
            }
        );
        assert_eq!(neighbors("a.mkv").previous, None);
        assert_eq!(neighbors("e.MP4").next, None);
        // A file that isn't media still has the ones around it by name.
        assert_eq!(neighbors("b.txt").next, Some(folder.join("c.mp3")));
        assert_eq!(
            Source::Url("https://example.com/live.m3u8".into()).neighbors(),
            Neighbors::default()
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn lists_media_files_and_links_to_them_but_not_folders() {
        let folder = std::env::temp_dir().join(format!("plays-media-in-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("season.mkv")).unwrap();
        for name in ["b.mp4", "notes.txt"] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(folder.join("b.mp4"), folder.join("a.mkv")).unwrap();
            std::os::unix::fs::symlink(folder.join("season.mkv"), folder.join("c.mkv")).unwrap();
            std::os::unix::fs::symlink(folder.join("gone.mp4"), folder.join("d.mp4")).unwrap();
        }
        let names: Vec<_> = media_in(&folder)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let expected: &[&str] = if cfg!(unix) {
            &["a.mkv", "b.mp4"]
        } else {
            &["b.mp4"]
        };
        assert_eq!(names, expected);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn names_and_displays_urls_without_secrets() {
        let source =
            Source::Url("https://user:secret@cdn.example.com:8443/live/cam1.m3u8?token=abc".into());
        assert_eq!(source.name(), "cdn.example.com:8443/cam1.m3u8");
        assert_eq!(
            source.to_string(),
            "https://cdn.example.com:8443/live/cam1.m3u8"
        );
        assert_eq!(
            Source::Url("rtmp://example.com".into()).name(),
            "example.com"
        );
        assert_eq!(Source::File("/videos/clip.mp4".into()).name(), "clip.mp4");
    }
}
