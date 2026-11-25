//! `audio`: a block's sound and its tracks, and every block's in a window.

use serde::Deserialize;
use serde_json::json;

use super::{Switch, done, select_track};
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Spec, on_mac};
use crate::api::target;
use crate::media::tracks::TrackKind;
use crate::ui::workspace::all;

/// How far the volume commands change a block's volume.
const VOLUME_STEP: f32 = 0.05;
/// How far the delay commands move the sound.
const DELAY_STEP: f64 = 0.05;

pub const COMMANDS: &[Spec] = &[
    Spec::block(
        "block.audio.volume_up",
        "Volume Up",
        "Turn the block's volume up, unmuting it.",
        |player, _, cx| player.change_volume(VOLUME_STEP, cx),
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["louder", "sound"])
    .setting("volume_up")
    .keys(&["up"]),
    Spec::block(
        "block.audio.volume_down",
        "Volume Down",
        "Turn the block's volume down.",
        |player, _, cx| player.change_volume(-VOLUME_STEP, cx),
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["quieter", "softer", "sound"])
    .setting("volume_down")
    .keys(&["down"]),
    Spec::block_with(
        "block.audio.set_volume",
        "Set Volume",
        "Set the block's volume, unmuting it unless it's none.",
        &[Arg::required(
            "volume",
            Kind::Number {
                min: 0.0,
                max: Some(100.0),
            },
            "How loud, from 0 to 100.",
        )],
        |args, player, _, cx| {
            let Volume { volume } = args.read()?;
            #[expect(clippy::cast_possible_truncation, reason = "0 to 1")]
            player.set_volume_to((volume / 100.0) as f32, cx);
            done()
        },
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["loudness", "sound", "level"]),
    Spec::block(
        "block.audio.mute.toggle",
        "Mute/Unmute",
        "Mute the block, or play its sound again.",
        |player, _, cx| player.turn_mute(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&["block.audio.mute.on", "block.audio.mute.off"])
    .keywords(&["silence", "sound"])
    .setting("mute")
    .keys(&["secondary-/", "m"])
    .state(|target, cx| target::read_block(target, cx, |player, _| json!(player.is_muted()))),
    Spec::block(
        "block.audio.mute.on",
        "Mute",
        "Mute the block, unless it's muted already.",
        |player, _, cx| player.turn_mute(|on| Switch::On.turn(on), cx),
    )
    .tells(&["block.audio.mute.on"])
    .keywords(&["silence", "sound"])
    .setting("audio.mute.on"),
    Spec::block(
        "block.audio.mute.off",
        "Unmute",
        "Play the block's sound again, unless it plays already.",
        |player, _, cx| player.turn_mute(|on| Switch::Off.turn(on), cx),
    )
    .tells(&["block.audio.mute.off"])
    .keywords(&["sound", "hear"])
    .setting("audio.mute.off"),
    Spec::block(
        "block.audio.next_track",
        "Next Audio Track",
        "Play the file's next audio track, the first after the last.",
        |player, _, cx| player.next_track(TrackKind::Audio, cx),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["language", "dub", "commentary"])
    .setting("next_audio_track")
    .keys(on_mac(&["ctrl-cmd-a"], &["ctrl-alt-a"])),
    Spec::block_with(
        "block.audio.select_track",
        "Pick Audio Track",
        "Play one of the file's audio tracks.",
        &[Arg::required(
            "number",
            Kind::Integer { min: 1 },
            "Which, from 1, as the tracks menu lists them.",
        )],
        |args, player, _, cx| select_track(args, TrackKind::Audio, player, cx),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["language", "dub", "commentary"]),
    Spec::block(
        "block.audio.earlier",
        "Audio Earlier",
        "Play the block's sound a little earlier than its picture.",
        |player, _, cx| player.shift_audio(-DELAY_STEP, cx),
    )
    .keywords(&["delay", "sync", "offset", "lip sync"])
    .setting("audio_earlier")
    .keys(&["ctrl--"]),
    Spec::block(
        "block.audio.later",
        "Audio Later",
        "Play the block's sound a little later than its picture.",
        |player, _, cx| player.shift_audio(DELAY_STEP, cx),
    )
    .keywords(&["delay", "sync", "offset", "lip sync"])
    .setting("audio_later")
    .keys(&["ctrl-="]),
    Spec::block(
        "block.audio.reset_delay",
        "Reset Audio Delay",
        "Play the block's sound with its picture again.",
        |player, _, cx| player.set_audio_delay(0.0, cx),
    )
    .keywords(&["sync", "offset"])
    .setting("reset_audio_delay")
    .keys(&["ctrl-backspace"]),
    Spec::every_block(
        "app.audio.volume_up",
        "Volume Up on All",
        "Turn every file's volume in every window up, each from its own.",
        |cx| all::change_volume(VOLUME_STEP, cx),
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["every", "louder"])
    .setting("volume_up_all")
    .keys(&["shift-up"]),
    Spec::every_block(
        "app.audio.volume_down",
        "Volume Down on All",
        "Turn every file's volume in every window down, each from its own.",
        |cx| all::change_volume(-VOLUME_STEP, cx),
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["every", "quieter"])
    .setting("volume_down_all")
    .keys(&["shift-down"]),
    Spec::every_block(
        "app.audio.mute.toggle",
        "Mute/Unmute All",
        "Mute every file in every window if any plays its sound, else unmute them all.",
        |cx| all::turn_mute(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&["block.audio.mute.on", "block.audio.mute.off"])
    .keywords(&["every", "silence"])
    .setting("mute_all")
    .keys(&["shift-m"]),
    Spec::every_block(
        "app.audio.mute.on",
        "Mute All",
        "Mute every file in every window.",
        |cx| all::turn_mute(|on| Switch::On.turn(on), cx),
    )
    .tells(&["block.audio.mute.on"])
    .keywords(&["every", "silence"])
    .setting("audio.mute_all.on"),
    Spec::every_block(
        "app.audio.mute.off",
        "Unmute All",
        "Play the sound of every file in every window again.",
        |cx| all::turn_mute(|on| Switch::Off.turn(on), cx),
    )
    .tells(&["block.audio.mute.off"])
    .keywords(&["every", "sound", "hear"])
    .setting("audio.mute_all.off"),
];

/// The blocks' events about their sound.
pub const EVENTS: &[EventKind] = &[
    EventKind::block(
        "block.audio.volume_changed",
        "A block's volume changed.",
        &[EventField::new(
            "volume",
            "Its volume, in percent, as `block.audio.set_volume` takes it.",
        )],
    ),
    EventKind::block("block.audio.mute.on", "A block was muted.", &[]),
    EventKind::block("block.audio.mute.off", "A block was unmuted.", &[])
        .causes(&["its volume turned up"]),
];

#[derive(Deserialize)]
struct Volume {
    volume: f64,
}
