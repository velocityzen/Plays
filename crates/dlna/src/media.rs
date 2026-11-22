//! What a renderer plays and what it's told about it: its protocol info,
//! read into capabilities; the DIDL-Lite that goes with a URL, subtitles in
//! the forms TVs look for; and what it says it's doing.
//!
//! What serves the media matters as much, as TVs are particular. A server
//! for them sends, on media:
//!
//! - `transferMode.dlna.org: Streaming` (or the mode asked for);
//! - `contentFeatures.dlna.org`, the same fourth field of protocol info as
//!   [`Media::features`]: `DLNA.ORG_OP=01` for a file it can seek in by
//!   bytes, `DLNA.ORG_OP=10` with `DLNA.ORG_CI=1` for a stream made as it
//!   goes, which it seeks in by time, and `DLNA.ORG_FLAGS`;
//! - for `TimeSeekRange.dlna.org: npt=…`, the stream from there, saying
//!   the range back;
//! - for `getcaptionInfo.sec: 1`, `CaptionInfo.sec` with the subtitles'
//!   URL, as Samsung TVs ask before playing.

use std::fmt;
use std::time::Instant;

use crate::xml::escape;

/// One entry of a renderer's protocol info: something it plays.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Capability {
    /// How it's fetched: `http-get` for the most part.
    pub protocol: String,
    pub network: String,
    /// Its MIME type, or `*`.
    pub mime: String,
    /// DLNA's name for its profile: `AVC_MP4_HP_HD_AAC`, `MPEG_TS_HD_NA`
    /// and so on.
    pub profile: Option<String>,
    /// The fourth field as it is: `*`, or DLNA's parameters.
    pub info: String,
}

impl Capability {
    /// One `protocol:network:mime:info` entry.
    pub fn parse(entry: &str) -> Option<Self> {
        let mut fields = entry.trim().splitn(4, ':');
        let protocol = fields.next()?.trim();
        let network = fields.next()?.trim();
        let mime = fields.next()?.trim();
        let info = fields.next()?.trim();
        if protocol.is_empty() || mime.is_empty() {
            return None;
        }
        let profile = info
            .split(';')
            .find_map(|parameter| parameter.trim().strip_prefix("DLNA.ORG_PN="))
            .map(str::to_owned);
        Some(Self {
            protocol: protocol.to_owned(),
            network: network.to_owned(),
            mime: mime.to_owned(),
            profile,
            info: info.to_owned(),
        })
    }

    /// A comma-separated list, as GetProtocolInfo gives it; a comma inside
    /// an entry is escaped with `\`.
    pub fn parse_list(list: &str) -> Vec<Self> {
        let mut entries = Vec::new();
        let mut entry = String::new();
        let mut chars = list.chars();
        while let Some(char) = chars.next() {
            match char {
                '\\' => entry.extend(chars.next()),
                ',' => entries.push(std::mem::take(&mut entry)),
                _ => entry.push(char),
            }
        }
        entries.push(entry);
        entries
            .iter()
            .filter_map(|entry| Self::parse(entry))
            .collect()
    }

    /// Over HTTP, which is what's served to it.
    pub fn is_http(&self) -> bool {
        self.protocol == "http-get"
    }

    /// Whether it takes `mime`, by name or `*`; parameters such as
    /// `audio/L16`'s rate aren't compared.
    pub fn takes(&self, mime: &str) -> bool {
        let essence = |mime: &str| {
            mime.split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
        };
        self.mime == "*" || essence(&self.mime) == essence(mime)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}",
            self.protocol, self.network, self.mime, self.info
        )
    }
}

/// What kind of thing is played, as DIDL-Lite classes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Video,
    Audio,
    Image,
}

impl Kind {
    fn class(self) -> &'static str {
        match self {
            Self::Video => "object.item.videoItem",
            Self::Audio => "object.item.audioItem.musicTrack",
            Self::Image => "object.item.imageItem.photo",
        }
    }
}

/// Subtitles, as a file the renderer fetches.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Subtitles {
    pub url: String,
    /// `text/srt` mostly, as TVs take.
    pub mime: String,
}

impl Subtitles {
    /// Its format as Samsung names it: `srt`, `smi` and so on.
    fn format(&self) -> &str {
        match self.mime.rsplit('/').next().unwrap_or_default() {
            "x-subrip" | "srt" => "srt",
            "x-sami" | "smi" | "sami" => "smi",
            other => other,
        }
    }
}

/// What to play.
#[derive(Clone, PartialEq, Debug)]
pub struct Media {
    pub url: String,
    /// Its MIME type: `video/mp4`, `video/mpeg` and so on.
    pub mime: String,
    /// DLNA's parameters for it, as its server says in
    /// `contentFeatures.dlna.org`; `*` when there are none.
    pub features: Option<String>,
    pub kind: Kind,
    pub title: Option<String>,
    /// Seconds, when known.
    pub duration: Option<f64>,
    /// Bytes, when known.
    pub size: Option<u64>,
    pub subtitles: Option<Subtitles>,
}

impl Media {
    /// Its protocol info, as `res` says it.
    pub fn protocol_info(&self) -> String {
        format!(
            "http-get:*:{}:{}",
            self.mime,
            self.features.as_deref().unwrap_or("*")
        )
    }

    /// The DIDL-Lite that goes with it in SetAVTransportURI: its title and
    /// class, the media as `res`, and subtitles, when there are any, both
    /// as a second `res` and as Samsung's `sec:CaptionInfoEx`.
    pub fn didl(&self) -> String {
        let title = self.title.clone().unwrap_or_else(|| {
            let path = self.url.split(['?', '#']).next().unwrap_or_default();
            path.rsplit('/').next().unwrap_or_default().to_owned()
        });
        let mut res = format!(" protocolInfo=\"{}\"", escape(&self.protocol_info()));
        if let Some(duration) = self.duration {
            res.push_str(&format!(" duration=\"{}\"", clock_exact(duration)));
        }
        if let Some(size) = self.size {
            res.push_str(&format!(" size=\"{size}\""));
        }
        let mut item = format!(
            "<dc:title>{}</dc:title><upnp:class>{}</upnp:class><res{res}>{}</res>",
            escape(&title),
            self.kind.class(),
            escape(&self.url)
        );
        if let Some(subtitles) = &self.subtitles {
            let url = escape(&subtitles.url);
            let format = escape(subtitles.format());
            item.push_str(&format!(
                "<res protocolInfo=\"http-get:*:{}:*\">{url}</res>\
                 <sec:CaptionInfoEx sec:type=\"{format}\">{url}</sec:CaptionInfoEx>\
                 <sec:CaptionInfo sec:type=\"{format}\">{url}</sec:CaptionInfo>",
                escape(&subtitles.mime)
            ));
        }
        format!(
            "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" \
             xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
             xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\" \
             xmlns:dlna=\"urn:schemas-dlna-org:metadata-1-0/\" \
             xmlns:sec=\"http://www.sec.co.kr/\">\
             <item id=\"0\" parentID=\"-1\" restricted=\"1\">{item}</item></DIDL-Lite>"
        )
    }
}

/// `H:MM:SS`, as Seek takes a time.
pub(crate) fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}:{:02}", whole / 3600, whole / 60 % 60, whole % 60)
}

/// `H:MM:SS.mmm`, as DIDL-Lite gives a duration.
pub(crate) fn clock_exact(seconds: f64) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!("{}.{:03}", clock(millis as f64 / 1000.0), millis % 1000)
}

/// Seconds from `H+:MM:SS[.F+]`, or `None` for what a renderer says when it
/// doesn't know: empty, `NOT_IMPLEMENTED` and the like.
pub(crate) fn seconds(clock: &str) -> Option<f64> {
    let clock = clock.trim();
    let mut parts = clock.split(':');
    let (hours, minutes, seconds) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let hours: f64 = hours.trim_start_matches('+').parse().ok()?;
    let minutes: f64 = minutes.parse().ok()?;
    let seconds: f64 = seconds.parse().ok()?;
    let total = hours * 3600.0 + minutes * 60.0 + seconds;
    total.is_finite().then_some(total)
}

/// What a renderer's transport is doing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
    /// Between one and another, as fetching what it was given.
    Transitioning,
    /// It has nothing to play.
    NoMedia,
    /// What else it may say, as it says it.
    Other(String),
}

impl TransportState {
    pub(crate) fn parse(state: &str) -> Self {
        match state.trim() {
            "STOPPED" => Self::Stopped,
            "PLAYING" => Self::Playing,
            "PAUSED_PLAYBACK" => Self::Paused,
            "TRANSITIONING" => Self::Transitioning,
            "NO_MEDIA_PRESENT" => Self::NoMedia,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// What it's doing with what it was given.
#[derive(Clone, PartialEq, Debug)]
pub struct Status {
    pub state: TransportState,
    /// Seconds in, when it says.
    pub position: Option<f64>,
    /// When it was first heard to be at `position`. Renderers move their
    /// position on about once a second, so it was truest then.
    pub position_at: Option<Instant>,
    /// Seconds, when it knows.
    pub duration: Option<f64>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            state: TransportState::NoMedia,
            position: None,
            position_at: None,
            duration: None,
        }
    }
}

impl Status {
    /// Be at `position`, heard `at`: it's news only if it moved.
    pub(crate) fn heard_at(&mut self, position: Option<f64>, at: Instant) {
        if position != self.position {
            self.position = position;
            self.position_at = position.map(|_| at);
        }
    }
}

/// What GetPositionInfo says.
#[derive(Clone, PartialEq, Debug)]
pub struct Position {
    /// What it plays, when it says.
    pub track_uri: Option<String>,
    pub position: Option<f64>,
    pub duration: Option<f64>,
}

/// A renderer's volume.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Volume {
    /// 0 to 1.
    pub level: f32,
    pub muted: bool,
}

#[cfg(test)]
#[path = "tests/media.rs"]
mod tests;
