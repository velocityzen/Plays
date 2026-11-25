//! What `block.ls` and `block.inspect` say of a block, as JSON: what it plays,
//! where it is, and how it's set to play; and what events see of it.

use serde_json::{Map, Value, json};

use super::PlayerView;
use crate::api::events::{BlockSeen, TRACKS};
use crate::cast::Device;
use crate::media::Source;
use crate::media::tracks::{Track, TrackKind};

/// The kinds of track, as `block.inspect` names them.
const KINDS: [(TrackKind, &str); 3] = [
    (TrackKind::Video, "video"),
    (TrackKind::Audio, "audio"),
    (TrackKind::Subtitle, "subtitles"),
];

impl PlayerView {
    /// Its id and label, its file's name, the file's path or the stream's
    /// URL, whether it plays, whether the file has opened, and why it
    /// couldn't, or couldn't play on.
    pub fn summary(&self) -> Map<String, Value> {
        let player = self.player();
        let (file, url) = match player.source() {
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
            "opened": player.is_known(),
            "error": player.error(),
        });
        let Value::Object(summary) = summary else {
            unreachable!("an object")
        };
        summary
    }

    /// All `block.inspect` says of it: its summary; where it is, how long
    /// it is, its video's frame rate and whether it ended; its volume (in
    /// percent), mute and loop; its tracks (numbered from 1 within each
    /// kind) and those picked; the delays and picture adjustments; its
    /// chapters (from 1) and the one it's in; the device it casts to; the
    /// output it plays through of its own, if any; and the panels it shows
    /// over its video.
    pub fn inspect(&self) -> Map<String, Value> {
        let player = self.player();
        let info = player.info();
        let tracks = |kind| -> Vec<&Track> {
            info.as_deref()
                .map(|info| info.tracks(kind).collect())
                .unwrap_or_default()
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
            "frame_rate": info.as_deref().and_then(|info| info.frame_rate),
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
                .map(|&(kind, name)| (name.to_owned(), json!(self.picked(kind))))
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
            "casting": self.casting().map(device_json),
            "output": self.sound_device(),
            "visible": self.over_video().visible(),
        });
        if let Value::Object(more) = more {
            inspected.extend(more);
        }
        inspected
    }

    /// The track of `kind` it plays, numbered from 1 among those of its
    /// kind.
    fn picked(&self, kind: TrackKind) -> Option<usize> {
        let info = self.player().info()?;
        let selected = info.selected(kind)?;
        let at = info
            .tracks(kind)
            .position(|track| track.index == selected)?;
        Some(at + 1)
    }

    /// What events see of it: what it plays, once it has opened, and how.
    pub fn seen(&self) -> BlockSeen {
        let player = self.player();
        let summary = self.summary();
        let opened = player.is_known().then(|| {
            json!({
                "title": summary["title"],
                "file": summary["file"],
                "url": summary["url"],
                "duration": self.duration(),
            })
        });
        let (seeks, sought) = self.playback.seeks();
        let track = |name| {
            let &(kind, _) = KINDS.iter().find(|&&(_, kind)| kind == name)?;
            self.picked(kind)
        };
        BlockSeen {
            id: self.id(),
            label: self.label().map(str::to_owned),
            title: self.header_title().to_string(),
            opening: self.playback.openings(),
            opened,
            failed: player.error(),
            playing: self.is_playing(),
            ended: self.has_ended(),
            seeks,
            sought,
            position: self.position(),
            duration: self.duration(),
            frame_rate: player.info().and_then(|info| info.frame_rate),
            volume: (f64::from(player.volume()) * 100.0).round(),
            muted: player.is_muted(),
            tracks: TRACKS.map(track),
            chapter: self.chapter().map(|at| at + 1),
            casting: self.casting().map(device_json),
            picture_in_picture: self.in_picture_in_picture(),
            over_video: self.over_video(),
        }
    }
}

/// A device it casts to, as `block.inspect` and events say.
fn device_json(device: &Device) -> Value {
    json!({
        "id": device.id,
        "kind": device.kind,
        "name": device.name,
    })
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
