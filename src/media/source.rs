//! What a player plays: a local file or a network stream.

use std::fmt;
use std::path::{Path, PathBuf};

/// URL schemes FFmpeg can stream from.
const STREAM_SCHEMES: &[&str] = &[
    "http", "https", "rtmp", "rtmps", "rtmpe", "rtmpt", "rtmpte", "rtmpts", "rtsp", "rtsps", "rtp",
    "srt", "udp", "tcp", "mms", "mmsh", "mmst", "ftp",
];

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
