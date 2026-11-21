//! `audio`: a tile's sound and its tracks, and every tile's in a window.

use serde_json::json;

use crate::api::registry::{Spec, on_mac};
use crate::api::target;
use crate::media::tracks::TrackKind;

/// How far the volume commands change a tile's volume.
const VOLUME_STEP: f32 = 0.05;
/// How far the delay commands move the sound.
const DELAY_STEP: f64 = 0.05;

pub const COMMANDS: &[Spec] = &[
    Spec::tile(
        "audio.volume_up",
        "Volume Up",
        "Turn the tile's volume up, unmuting it.",
        |player, _, cx| player.change_volume(VOLUME_STEP, cx),
    )
    .keywords(&["louder", "sound"])
    .setting("volume_up")
    .keys(&["up"]),
    Spec::tile(
        "audio.volume_down",
        "Volume Down",
        "Turn the tile's volume down.",
        |player, _, cx| player.change_volume(-VOLUME_STEP, cx),
    )
    .keywords(&["quieter", "softer", "sound"])
    .setting("volume_down")
    .keys(&["down"]),
    Spec::tile(
        "audio.toggle_mute",
        "Mute/Unmute",
        "Mute the tile, or play its sound again.",
        |player, _, cx| player.toggle_mute(cx),
    )
    .keywords(&["silence", "sound"])
    .setting("mute")
    .keys(&["secondary-/", "m"])
    .state(|target, cx| {
        target::read_tile(target, cx, |player, _| json!(player.player().is_muted()))
    }),
    Spec::tile(
        "audio.next_track",
        "Next Audio Track",
        "Play the file's next audio track, the first after the last.",
        |player, _, cx| player.next_track(TrackKind::Audio, cx),
    )
    .keywords(&["language", "dub", "commentary"])
    .setting("next_audio_track")
    .keys(on_mac(&["ctrl-cmd-a"], &["ctrl-alt-a"])),
    Spec::tile(
        "audio.earlier",
        "Audio Earlier",
        "Play the tile's sound a little earlier than its picture.",
        |player, _, cx| player.shift_audio(-DELAY_STEP, cx),
    )
    .keywords(&["delay", "sync", "offset", "lip sync"])
    .setting("audio_earlier")
    .keys(&["ctrl--"]),
    Spec::tile(
        "audio.later",
        "Audio Later",
        "Play the tile's sound a little later than its picture.",
        |player, _, cx| player.shift_audio(DELAY_STEP, cx),
    )
    .keywords(&["delay", "sync", "offset", "lip sync"])
    .setting("audio_later")
    .keys(&["ctrl-="]),
    Spec::tile(
        "audio.reset_delay",
        "Reset Audio Delay",
        "Play the tile's sound with its picture again.",
        |player, _, cx| player.set_audio_delay(0.0, cx),
    )
    .keywords(&["sync", "offset"])
    .setting("reset_audio_delay")
    .keys(&["ctrl-backspace"]),
    Spec::tiles(
        "audio.volume_up_all",
        "Volume Up on All",
        "Turn every file's volume in the window up, each from its own.",
        |workspace, _, cx| workspace.change_volume_all(VOLUME_STEP, cx),
    )
    .keywords(&["every", "louder"])
    .setting("volume_up_all")
    .keys(&["shift-up"]),
    Spec::tiles(
        "audio.volume_down_all",
        "Volume Down on All",
        "Turn every file's volume in the window down, each from its own.",
        |workspace, _, cx| workspace.change_volume_all(-VOLUME_STEP, cx),
    )
    .keywords(&["every", "quieter"])
    .setting("volume_down_all")
    .keys(&["shift-down"]),
    Spec::tiles(
        "audio.toggle_mute_all",
        "Mute/Unmute All",
        "Mute every file in the window if any plays its sound, else unmute them all.",
        |workspace, _, cx| workspace.toggle_mute_all(cx),
    )
    .keywords(&["every", "silence"])
    .setting("mute_all")
    .keys(&["shift-m"]),
];
