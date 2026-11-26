//! `audio`: a block's sound and its tracks; and every block's in a window,
//! or in every window.

use gpui::App;
use serde::Deserialize;
use serde_json::json;

use super::{Switch, done, select_track, select_tracks};
use crate::api::Error;
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Args, Outcome, Spec, on_mac};
use crate::api::target;
use crate::media::tracks::TrackKind;
use crate::ui::workspace::all::Players;

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
        VOLUME,
        |args, player, _, cx| {
            player.set_volume_to(volume(args)?, cx);
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
        TRACK,
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
    Spec::blocks(
        "window.audio.volume_up",
        "Volume Up in Window",
        "Turn every file in the window up, each from its own volume, unmuting it.",
        |players, cx| players.each(cx, |player, cx| player.change_volume(VOLUME_STEP, cx)),
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["every", "all", "louder", "sound"]),
    Spec::blocks(
        "window.audio.volume_down",
        "Volume Down in Window",
        "Turn every file in the window down, each from its own volume.",
        |players, cx| players.each(cx, |player, cx| player.change_volume(-VOLUME_STEP, cx)),
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["every", "all", "quieter", "softer", "sound"]),
    Spec::blocks_with(
        "window.audio.set_volume",
        "Set Volume in Window",
        "Set the volume of every file in the window, unmuting them unless it's none.",
        VOLUME,
        set_volume_of_each,
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["every", "all", "loudness", "sound", "level"]),
    Spec::blocks(
        "window.audio.mute.toggle",
        "Mute/Unmute in Window",
        "Mute every file in the window if any plays its sound, else unmute them all.",
        |players, cx| players.turn_mute(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&["block.audio.mute.on", "block.audio.mute.off"])
    .keywords(&["every", "all", "silence", "sound"]),
    Spec::blocks(
        "window.audio.mute.on",
        "Mute in Window",
        "Mute every file in the window.",
        |players, cx| players.turn_mute(|on| Switch::On.turn(on), cx),
    )
    .tells(&["block.audio.mute.on"])
    .keywords(&["every", "all", "silence", "sound"]),
    Spec::blocks(
        "window.audio.mute.off",
        "Unmute in Window",
        "Play the sound of every file in the window again.",
        |players, cx| players.turn_mute(|on| Switch::Off.turn(on), cx),
    )
    .tells(&["block.audio.mute.off"])
    .keywords(&["every", "all", "sound", "hear"]),
    Spec::blocks(
        "window.audio.next_track",
        "Next Audio Track in Window",
        "Play the next audio track of every file in the window, the first after the last.",
        |players, cx| players.each(cx, |player, cx| player.next_track(TrackKind::Audio, cx)),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["every", "all", "language", "dub", "commentary"]),
    Spec::blocks_with(
        "window.audio.select_track",
        "Pick Audio Track in Window",
        "Play an audio track of every file in the window that has it.",
        TRACK,
        |args, players, cx| select_tracks(args, TrackKind::Audio, players, cx),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["every", "all", "language", "dub", "commentary"]),
    Spec::blocks(
        "window.audio.earlier",
        "Audio Earlier in Window",
        "Play the sound of every file in the window a little earlier than its picture.",
        |players, cx| players.each(cx, |player, cx| player.shift_audio(-DELAY_STEP, cx)),
    )
    .keywords(&["every", "all", "delay", "sync", "offset", "lip sync"]),
    Spec::blocks(
        "window.audio.later",
        "Audio Later in Window",
        "Play the sound of every file in the window a little later than its picture.",
        |players, cx| players.each(cx, |player, cx| player.shift_audio(DELAY_STEP, cx)),
    )
    .keywords(&["every", "all", "delay", "sync", "offset", "lip sync"]),
    Spec::blocks(
        "window.audio.reset_delay",
        "Reset Audio Delay in Window",
        "Play the sound of every file in the window with its picture again.",
        |players, cx| players.each(cx, |player, cx| player.set_audio_delay(0.0, cx)),
    )
    .keywords(&["every", "all", "sync", "offset"]),
    Spec::every_block(
        "app.audio.volume_up",
        "Volume Up in Every Window",
        "Turn every file in every window up, each from its own volume, unmuting it.",
        |players, cx| players.each(cx, |player, cx| player.change_volume(VOLUME_STEP, cx)),
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["every", "all", "louder", "sound"])
    .setting("volume_up_all")
    .keys(&["shift-up"]),
    Spec::every_block(
        "app.audio.volume_down",
        "Volume Down in Every Window",
        "Turn every file in every window down, each from its own volume.",
        |players, cx| players.each(cx, |player, cx| player.change_volume(-VOLUME_STEP, cx)),
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["every", "all", "quieter", "softer", "sound"])
    .setting("volume_down_all")
    .keys(&["shift-down"]),
    Spec::every_block_with(
        "app.audio.set_volume",
        "Set Volume in Every Window",
        "Set the volume of every file in every window, unmuting them unless it's none.",
        VOLUME,
        set_volume_of_each,
    )
    .tells(&["block.audio.volume_changed"])
    .keywords(&["every", "all", "loudness", "sound", "level"]),
    Spec::every_block(
        "app.audio.mute.toggle",
        "Mute/Unmute in Every Window",
        "Mute every file in every window if any plays its sound, else unmute them all.",
        |players, cx| players.turn_mute(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&["block.audio.mute.on", "block.audio.mute.off"])
    .keywords(&["every", "all", "silence", "sound"])
    .setting("mute_all")
    .keys(&["shift-m"]),
    Spec::every_block(
        "app.audio.mute.on",
        "Mute in Every Window",
        "Mute every file in every window.",
        |players, cx| players.turn_mute(|on| Switch::On.turn(on), cx),
    )
    .tells(&["block.audio.mute.on"])
    .keywords(&["every", "all", "silence", "sound"])
    .setting("audio.mute_all.on"),
    Spec::every_block(
        "app.audio.mute.off",
        "Unmute in Every Window",
        "Play the sound of every file in every window again.",
        |players, cx| players.turn_mute(|on| Switch::Off.turn(on), cx),
    )
    .tells(&["block.audio.mute.off"])
    .keywords(&["every", "all", "sound", "hear"])
    .setting("audio.mute_all.off"),
    Spec::every_block(
        "app.audio.next_track",
        "Next Audio Track in Every Window",
        "Play the next audio track of every file in every window, the first after the last.",
        |players, cx| players.each(cx, |player, cx| player.next_track(TrackKind::Audio, cx)),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["every", "all", "language", "dub", "commentary"]),
    Spec::every_block_with(
        "app.audio.select_track",
        "Pick Audio Track in Every Window",
        "Play an audio track of every file in every window that has it.",
        TRACK,
        |args, players, cx| select_tracks(args, TrackKind::Audio, players, cx),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["every", "all", "language", "dub", "commentary"]),
    Spec::every_block(
        "app.audio.earlier",
        "Audio Earlier in Every Window",
        "Play the sound of every file in every window a little earlier than its picture.",
        |players, cx| players.each(cx, |player, cx| player.shift_audio(-DELAY_STEP, cx)),
    )
    .keywords(&["every", "all", "delay", "sync", "offset", "lip sync"]),
    Spec::every_block(
        "app.audio.later",
        "Audio Later in Every Window",
        "Play the sound of every file in every window a little later than its picture.",
        |players, cx| players.each(cx, |player, cx| player.shift_audio(DELAY_STEP, cx)),
    )
    .keywords(&["every", "all", "delay", "sync", "offset", "lip sync"]),
    Spec::every_block(
        "app.audio.reset_delay",
        "Reset Audio Delay in Every Window",
        "Play the sound of every file in every window with its picture again.",
        |players, cx| players.each(cx, |player, cx| player.set_audio_delay(0.0, cx)),
    )
    .keywords(&["every", "all", "sync", "offset"]),
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

/// A volume to set.
const VOLUME: &[Arg] = &[Arg::required(
    "volume",
    Kind::Number {
        min: 0.0,
        max: Some(100.0),
    },
    "How loud, from 0 to 100.",
)];

/// An audio track to play.
const TRACK: &[Arg] = &[Arg::required(
    "number",
    Kind::Integer { min: 1 },
    "Which, from 1, as the tracks menu lists them.",
)];

#[derive(Deserialize)]
struct Volume {
    volume: f64,
}

/// The volume `args` set, from 0 to 1.
fn volume(args: Args) -> Result<f32, Error> {
    let Volume { volume } = args.read()?;
    #[expect(clippy::cast_possible_truncation, reason = "0 to 1")]
    Ok((volume / 100.0) as f32)
}

/// Set each one's volume as `args` say.
fn set_volume_of_each(args: Args, players: &Players, cx: &mut App) -> Outcome {
    let volume = volume(args)?;
    players.each(cx, |player, cx| player.set_volume_to(volume, cx));
    done()
}
