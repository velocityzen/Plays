//! What `tile.ls` and `tile.inspect` say of a tile, as JSON: what it plays,
//! where it is, and how it's set to play.

use serde_json::{Map, Value, json};

use super::PlayerView;
use crate::media::Source;
use crate::media::tracks::{Track, TrackKind};

/// The kinds of track, as `tile.inspect` names them.
const KINDS: [(TrackKind, &str); 3] = [
    (TrackKind::Video, "video"),
    (TrackKind::Audio, "audio"),
    (TrackKind::Subtitle, "subtitles"),
];

impl PlayerView {
    /// Its id and label, its file's name, the file's path or the stream's
    /// URL, and whether it plays.
    pub fn summary(&self) -> Map<String, Value> {
        let (file, url) = match self.player().source() {
            Source::File(path) => (Some(path.display().to_string()), None),
            // As shown: without a user, password or query.
            source @ Source::Url(_) => (None, Some(source.to_string())),
        };
        let summary = json!({
            "id": self.id().to_string(),
            "label": self.label(),
            "title": self.title.to_string(),
            "file": file,
            "url": url,
            "playing": self.is_playing(),
        });
        let Value::Object(summary) = summary else {
            unreachable!("an object")
        };
        summary
    }

    /// All `tile.inspect` says of it: its summary; where it is, how long
    /// it is and whether it ended; its volume (in percent), mute and loop; its tracks
    /// (numbered from 1 within each kind) and those picked; the delays and
    /// picture adjustments; its chapters (from 1) and the one it's in; and
    /// the device it casts to.
    pub fn inspect(&self) -> Map<String, Value> {
        let player = self.player();
        let info = player.info();
        let tracks = |kind| -> Vec<&Track> {
            info.as_deref()
                .map(|info| info.tracks(kind).collect())
                .unwrap_or_default()
        };
        let picked = |kind| {
            let selected = info.as_deref()?.selected(kind)?;
            let at = tracks(kind)
                .iter()
                .position(|track| track.index == selected)?;
            Some(at + 1)
        };
        let adjustments = self.adjustments();
        let chapters: Vec<Value> = self
            .chapters()
            .iter()
            .enumerate()
            .map(|(at, chapter)| {
                json!({
                    "number": at + 1,
                    "name": chapter.name(at),
                    "start": chapter.start,
                    "end": chapter.end,
                })
            })
            .collect();
        let mut inspected = self.summary();
        let more = json!({
            "position": self.position(),
            "duration": self.duration(),
            "ended": self.playback.has_ended(),
            // In percent, as `audio.set_volume` takes it.
            "volume": (f64::from(player.volume()) * 100.0).round(),
            "muted": player.is_muted(),
            "looping": player.is_looping(),
            "tracks": KINDS
                .iter()
                .map(|&(kind, name)| {
                    let listed: Vec<Value> = tracks(kind)
                        .iter()
                        .enumerate()
                        .map(|(at, track)| track_json(track, at + 1))
                        .collect();
                    (name.to_owned(), Value::from(listed))
                })
                .collect::<Map<_, _>>(),
            "picked": KINDS
                .iter()
                .map(|&(kind, name)| (name.to_owned(), json!(picked(kind))))
                .collect::<Map<_, _>>(),
            "delays": {
                "audio": milliseconds(player.audio_delay()),
                "subtitles": milliseconds(player.subtitle_delay()),
            },
            "picture": {
                "turns": adjustments.orientation.clockwise_turns(),
                "mirrored": adjustments.orientation.is_mirrored(),
                "aspect_ratio": adjustments.aspect.map(|ratio| ratio.name),
                "crop": adjustments.crop.map(|ratio| ratio.name),
            },
            "chapter": self.chapter().map(|at| at + 1),
            "chapters": chapters,
            "casting": self.casting().map(|device| json!({
                "id": device.id,
                "kind": device.kind,
                "name": device.name,
            })),
        });
        if let Value::Object(more) = more {
            inspected.extend(more);
        }
        inspected
    }
}

/// `seconds`, to the millisecond: a delay kept as `f32` shows as it was
/// set.
fn milliseconds(seconds: f64) -> f64 {
    (seconds * 1000.0).round() / 1000.0
}

/// A track, numbered `number` among those of its kind.
fn track_json(track: &Track, number: usize) -> Value {
    json!({
        "number": number,
        "label": track.label(number),
        "language": track.language,
        "title": track.title,
        "codec": track.codec,
        "format": track.format,
        "default": track.default,
        "forced": track.forced,
        "hearing_impaired": track.hearing_impaired,
        "file": track.file.as_ref().map(|file| file.display().to_string()),
    })
}
