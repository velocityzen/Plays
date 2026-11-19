//! What's cast, as the Default Media Receiver takes it: the URL and what
//! it is, its subtitle tracks and how they look; and the status it gives
//! back.

use serde_json::{Map, Value, json};

/// What to play.
#[derive(Clone, PartialEq, Debug)]
pub struct Media {
    pub url: String,
    /// Its MIME type: `video/mp4`, `application/x-mpegURL` and so on.
    pub content_type: String,
    pub stream_type: StreamType,
    pub title: Option<String>,
    /// Seconds, when known.
    pub duration: Option<f64>,
    pub text_tracks: Vec<TextTrack>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamType {
    /// A file, or a stream with an end: it can be sought.
    Buffered,
    Live,
}

/// A subtitle track, as a file the device fetches.
#[derive(Clone, PartialEq, Debug)]
pub struct TextTrack {
    pub id: u32,
    pub url: String,
    /// `text/vtt`, as the Default Media Receiver takes.
    pub content_type: String,
    pub name: String,
    /// Its language, as a BCP 47 tag.
    pub language: Option<String>,
}

/// An RGBA color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color(pub [u8; 4]);

impl Color {
    pub const WHITE: Self = Self([0xFF, 0xFF, 0xFF, 0xFF]);
    pub const BLACK: Self = Self([0, 0, 0, 0xFF]);
    pub const CLEAR: Self = Self([0, 0, 0, 0]);

    /// `#RRGGBBAA`, as Cast says it.
    fn hex(self) -> String {
        let [r, g, b, a] = self.0;
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    None,
    Outline,
    DropShadow,
    Raised,
    Depressed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontFamily {
    SansSerif,
    MonospacedSansSerif,
    Serif,
    MonospacedSerif,
    Casual,
    Cursive,
    SmallCapitals,
}

/// How subtitles look.
#[derive(Clone, PartialEq, Debug)]
pub struct TextTrackStyle {
    pub foreground: Color,
    pub background: Color,
    pub edge: Edge,
    pub edge_color: Color,
    /// 1 is the device's own size.
    pub font_scale: f32,
    /// A font by name, which the device may not have; `generic_family` is
    /// the fallback.
    pub font_family: Option<String>,
    pub generic_family: FontFamily,
    pub bold: bool,
    pub italic: bool,
}

impl Default for TextTrackStyle {
    fn default() -> Self {
        Self {
            foreground: Color::WHITE,
            background: Color::CLEAR,
            edge: Edge::Outline,
            edge_color: Color::BLACK,
            font_scale: 1.0,
            font_family: None,
            generic_family: FontFamily::SansSerif,
            bold: false,
            italic: false,
        }
    }
}

impl TextTrackStyle {
    pub(crate) fn json(&self) -> Value {
        let edge = match self.edge {
            Edge::None => "NONE",
            Edge::Outline => "OUTLINE",
            Edge::DropShadow => "DROP_SHADOW",
            Edge::Raised => "RAISED",
            Edge::Depressed => "DEPRESSED",
        };
        let generic = match self.generic_family {
            FontFamily::SansSerif => "SANS_SERIF",
            FontFamily::MonospacedSansSerif => "MONOSPACED_SANS_SERIF",
            FontFamily::Serif => "SERIF",
            FontFamily::MonospacedSerif => "MONOSPACED_SERIF",
            FontFamily::Casual => "CASUAL",
            FontFamily::Cursive => "CURSIVE",
            FontFamily::SmallCapitals => "SMALL_CAPITALS",
        };
        let font_style = match (self.bold, self.italic) {
            (false, false) => "NORMAL",
            (true, false) => "BOLD",
            (false, true) => "ITALIC",
            (true, true) => "BOLD_ITALIC",
        };
        let mut style = json!({
            "foregroundColor": self.foreground.hex(),
            "backgroundColor": self.background.hex(),
            "edgeType": edge,
            "edgeColor": self.edge_color.hex(),
            "fontScale": self.font_scale,
            "fontGenericFamily": generic,
            "fontStyle": font_style,
            "windowType": "NONE",
        });
        if let Some(family) = &self.font_family {
            style["fontFamily"] = family.as_str().into();
        }
        style
    }
}

impl Media {
    /// As LOAD takes it, with how its subtitles look.
    pub(crate) fn json(&self, style: Option<&TextTrackStyle>) -> Value {
        let mut media = Map::new();
        media.insert("contentId".into(), self.url.as_str().into());
        media.insert("contentUrl".into(), self.url.as_str().into());
        media.insert("contentType".into(), self.content_type.as_str().into());
        let stream_type = match self.stream_type {
            StreamType::Buffered => "BUFFERED",
            StreamType::Live => "LIVE",
        };
        media.insert("streamType".into(), stream_type.into());
        let mut metadata = json!({"metadataType": 0});
        if let Some(title) = &self.title {
            metadata["title"] = title.as_str().into();
        }
        media.insert("metadata".into(), metadata);
        if let Some(duration) = self.duration {
            media.insert("duration".into(), duration.into());
        }
        if !self.text_tracks.is_empty() {
            let tracks = self
                .text_tracks
                .iter()
                .map(|track| {
                    let mut json = json!({
                        "trackId": track.id,
                        "type": "TEXT",
                        "subtype": "SUBTITLES",
                        "trackContentId": track.url,
                        "trackContentType": track.content_type,
                        "name": track.name,
                    });
                    if let Some(language) = &track.language {
                        json["language"] = language.as_str().into();
                    }
                    json
                })
                .collect();
            media.insert("tracks".into(), Value::Array(tracks));
        }
        if let Some(style) = style {
            media.insert("textTrackStyle".into(), style.json());
        }
        Value::Object(media)
    }
}

/// What the device is doing with what it was given.
#[derive(Clone, PartialEq, Debug)]
pub struct MediaStatus {
    pub state: PlayerState,
    /// Seconds in.
    pub position: f64,
    /// Seconds, once it knows; a live stream has none.
    pub duration: Option<f64>,
    /// The tracks shown.
    pub active_tracks: Vec<u32>,
    /// The device's id for what it plays, which changes with each LOAD.
    pub media_session: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayerState {
    /// Nothing playing, and why, when it says.
    Idle(Option<IdleReason>),
    /// Getting it, or waiting for more of it.
    Buffering,
    Playing,
    Paused,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleReason {
    /// Played to the end.
    Finished,
    /// Stopped by a sender.
    Cancelled,
    /// Replaced by something another sender loaded.
    Interrupted,
    /// It couldn't play it.
    Error,
}

impl MediaStatus {
    /// One entry of MEDIA_STATUS's `status`.
    pub(crate) fn read(status: &Value) -> Option<Self> {
        let media_session = status["mediaSessionId"].as_u64()?;
        let state = match status["playerState"].as_str()? {
            "PLAYING" => PlayerState::Playing,
            "PAUSED" => PlayerState::Paused,
            "BUFFERING" | "LOADING" => PlayerState::Buffering,
            _ => PlayerState::Idle(match status["idleReason"].as_str() {
                Some("FINISHED") => Some(IdleReason::Finished),
                Some("CANCELLED") => Some(IdleReason::Cancelled),
                Some("INTERRUPTED") => Some(IdleReason::Interrupted),
                Some("ERROR") => Some(IdleReason::Error),
                _ => None,
            }),
        };
        Some(Self {
            state,
            position: status["currentTime"].as_f64().unwrap_or(0.0),
            duration: status["media"]["duration"]
                .as_f64()
                .filter(|duration| *duration > 0.0),
            active_tracks: status["activeTrackIds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| u32::try_from(id.as_u64()?).ok())
                .collect(),
            media_session,
        })
    }
}

/// The device's volume.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Volume {
    /// 0 to 1.
    pub level: f32,
    pub muted: bool,
}

impl Volume {
    /// RECEIVER_STATUS's `status.volume`.
    pub(crate) fn read(volume: &Value) -> Option<Self> {
        Some(Self {
            level: volume["level"].as_f64()? as f32,
            muted: volume["muted"].as_bool().unwrap_or(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_with_subtitles_as_load_takes_it() {
        let media = Media {
            url: "http://192.0.2.1:8080/a/master.m3u8".into(),
            content_type: "application/x-mpegURL".into(),
            stream_type: StreamType::Buffered,
            title: Some("A Film".into()),
            duration: Some(5400.0),
            text_tracks: vec![TextTrack {
                id: 1,
                url: "http://192.0.2.1:8080/a/subs.vtt".into(),
                content_type: "text/vtt".into(),
                name: "English".into(),
                language: Some("en".into()),
            }],
        };
        let style = TextTrackStyle {
            font_scale: 1.25,
            italic: true,
            ..TextTrackStyle::default()
        };
        let json = media.json(Some(&style));
        assert_eq!(json["contentId"], media.url);
        assert_eq!(json["streamType"], "BUFFERED");
        assert_eq!(json["metadata"]["title"], "A Film");
        assert_eq!(json["duration"], 5400.0);
        assert_eq!(
            json["tracks"][0]["trackContentId"],
            "http://192.0.2.1:8080/a/subs.vtt"
        );
        assert_eq!(json["tracks"][0]["language"], "en");
        let style = &json["textTrackStyle"];
        assert_eq!(style["foregroundColor"], "#FFFFFFFF");
        assert_eq!(style["backgroundColor"], "#00000000");
        assert_eq!(style["edgeType"], "OUTLINE");
        assert_eq!(style["fontScale"], 1.25);
        assert_eq!(style["fontStyle"], "ITALIC");
        assert!(style.get("fontFamily").is_none());
    }

    #[test]
    fn reads_what_the_device_says_it_is_doing() {
        let status = json!({
            "mediaSessionId": 3,
            "playerState": "PLAYING",
            "currentTime": 61.5,
            "activeTrackIds": [1],
            "media": {"duration": 5400.0},
        });
        assert_eq!(
            MediaStatus::read(&status),
            Some(MediaStatus {
                state: PlayerState::Playing,
                position: 61.5,
                duration: Some(5400.0),
                active_tracks: vec![1],
                media_session: 3,
            })
        );
        let finished =
            json!({"mediaSessionId": 3, "playerState": "IDLE", "idleReason": "FINISHED"});
        assert_eq!(
            MediaStatus::read(&finished).unwrap().state,
            PlayerState::Idle(Some(IdleReason::Finished))
        );
        assert_eq!(MediaStatus::read(&json!({"playerState": "PLAYING"})), None);
    }
}
