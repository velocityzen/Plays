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
    /// Seconds, when it knows.
    pub duration: Option<f64>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            state: TransportState::NoMedia,
            position: None,
            duration: None,
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
mod tests {
    use super::*;
    use crate::xml::Element;

    #[test]
    fn protocol_info_entries_and_their_profiles() {
        let mp4 = Capability::parse(
            "http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD_AAC;DLNA.ORG_FLAGS=8d70",
        )
        .unwrap();
        assert_eq!(mp4.protocol, "http-get");
        assert_eq!(mp4.mime, "video/mp4");
        assert_eq!(mp4.profile.as_deref(), Some("AVC_MP4_HP_HD_AAC"));
        assert!(mp4.is_http() && mp4.takes("VIDEO/MP4") && !mp4.takes("video/mpeg"));
        assert_eq!(
            mp4.to_string(),
            "http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD_AAC;DLNA.ORG_FLAGS=8d70"
        );
        let any = Capability::parse("x-rincon:*:*:*").unwrap();
        assert!(any.takes("audio/flac") && !any.is_http());
        assert_eq!(any.profile, None);
        assert_eq!(Capability::parse("http-get:*:video/mp4"), None);
        assert_eq!(Capability::parse(""), None);
        let list = Capability::parse_list(
            " http-get:*:audio/mpeg:DLNA.ORG_PN=MP3, ,http-get:*:x/odd:a=1\\,2,broken",
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].info, "a=1,2");
    }

    #[test]
    fn protocol_info_from_real_renderers() {
        let samsung = Capability::parse_list(include_str!("../fixtures/samsung-protocol-info.txt"));
        assert_eq!(samsung.len(), 18);
        assert!(samsung.iter().all(Capability::is_http));
        assert!(
            samsung
                .iter()
                .any(|it| it.mime == "video/mpeg" && it.profile.as_deref() == Some("MPEG_TS_HD_NA"))
        );
        assert!(samsung.iter().any(|it| it.takes("text/srt")));
        let lg = Capability::parse_list(include_str!("../fixtures/lg-protocol-info.txt"));
        assert_eq!(lg.len(), 11);
        assert_eq!(lg.iter().filter(|it| !it.is_http()).count(), 1);
        assert!(
            lg.iter()
                .any(|it| it.takes("video/x-matroska") && it.profile.is_none())
        );
        let devialet =
            Capability::parse_list(include_str!("../fixtures/devialet-protocol-info.txt"));
        assert_eq!(devialet.len(), 57);
        assert!(
            devialet
                .iter()
                .all(|it| it.is_http() && it.profile.is_none())
        );
        assert!(
            devialet
                .iter()
                .any(|it| it.takes("audio/L16;rate=48000;channels=2"))
        );
        assert!(
            devialet
                .iter()
                .any(|it| it.mime == "audio/l16;channels=2;rate=44100")
        );
        assert!(!devialet.iter().any(|it| it.mime.starts_with("video/")));
        let sonos = Capability::parse_list(include_str!("../fixtures/sonos-protocol-info.txt"));
        assert!(
            sonos
                .iter()
                .any(|it| it.is_http() && it.takes("audio/flac"))
        );
        assert!(
            !sonos
                .iter()
                .any(|it| it.is_http() && it.mime.starts_with("video/"))
        );
    }

    fn film() -> Media {
        Media {
            url: "http://192.0.2.1:8080/cast/media?x=1&y=2".into(),
            mime: "video/mpeg".into(),
            features: Some("DLNA.ORG_OP=10;DLNA.ORG_CI=1".into()),
            kind: Kind::Video,
            title: Some("Tom & Jerry".into()),
            duration: Some(5400.25),
            size: None,
            subtitles: Some(Subtitles {
                url: "http://192.0.2.1:8080/cast/subs.srt".into(),
                mime: "text/srt".into(),
            }),
        }
    }

    #[test]
    fn didl_lite_with_subtitles_both_ways() {
        let didl = Element::parse(&film().didl()).unwrap();
        let item = didl.child("item").unwrap();
        assert_eq!(item.child_text("title"), Some("Tom & Jerry"));
        assert_eq!(item.child_text("class"), Some("object.item.videoItem"));
        let res: Vec<&Element> = item.children("res").collect();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].text(), "http://192.0.2.1:8080/cast/media?x=1&y=2");
        assert_eq!(
            res[0].attribute("protocolInfo"),
            Some("http-get:*:video/mpeg:DLNA.ORG_OP=10;DLNA.ORG_CI=1")
        );
        assert_eq!(res[0].attribute("duration"), Some("1:30:00.250"));
        assert_eq!(res[0].attribute("size"), None);
        assert_eq!(res[1].text(), "http://192.0.2.1:8080/cast/subs.srt");
        assert_eq!(
            res[1].attribute("protocolInfo"),
            Some("http-get:*:text/srt:*")
        );
        let caption = item.child("CaptionInfoEx").unwrap();
        assert_eq!(caption.text(), "http://192.0.2.1:8080/cast/subs.srt");
        assert_eq!(caption.attribute("type"), Some("srt"));
        assert!(item.child("CaptionInfo").is_some());
    }

    #[test]
    fn didl_lite_without_subtitles_or_title() {
        let song = Media {
            url: "http://192.0.2.1:8080/music/song.flac?token=1".into(),
            mime: "audio/flac".into(),
            features: None,
            kind: Kind::Audio,
            title: None,
            duration: None,
            size: Some(123),
            subtitles: None,
        };
        let didl = Element::parse(&song.didl()).unwrap();
        let item = didl.child("item").unwrap();
        assert_eq!(item.child_text("title"), Some("song.flac"));
        assert_eq!(item.children("res").count(), 1);
        assert!(item.child("CaptionInfoEx").is_none());
        let res = item.child("res").unwrap();
        assert_eq!(
            res.attribute("protocolInfo"),
            Some("http-get:*:audio/flac:*")
        );
        assert_eq!(res.attribute("size"), Some("123"));
        assert_eq!(res.attribute("duration"), None);
    }

    #[test]
    fn times_as_renderers_say_them() {
        assert_eq!(clock(3723.9), "1:02:03");
        assert_eq!(clock(-5.0), "0:00:00");
        assert_eq!(clock_exact(59.9996), "0:01:00.000");
        assert_eq!(clock_exact(61.5), "0:01:01.500");
        assert_eq!(seconds("1:02:03"), Some(3723.0));
        assert_eq!(seconds("00:00:01.500"), Some(1.5));
        assert_eq!(seconds("+10:00:00"), Some(36000.0));
        assert_eq!(seconds("NOT_IMPLEMENTED"), None);
        assert_eq!(seconds(""), None);
        assert_eq!(seconds("1:2:3:4"), None);
    }

    #[test]
    fn transport_states() {
        assert_eq!(
            TransportState::parse("PAUSED_PLAYBACK"),
            TransportState::Paused
        );
        assert_eq!(TransportState::parse(" PLAYING "), TransportState::Playing);
        assert_eq!(
            TransportState::parse("NO_MEDIA_PRESENT"),
            TransportState::NoMedia
        );
        assert_eq!(
            TransportState::parse("RECORDING"),
            TransportState::Other("RECORDING".into())
        );
    }
}
