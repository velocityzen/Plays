//! `subtitles`: a block's subtitles, and how every file's look; and every
//! block's in a window, or in every window.

use gpui::Context;

use serde_json::json;

use super::{Switch, select_track, select_tracks};
use crate::api::args::{Arg, Kind};
use crate::api::events::EventKind;
use crate::api::registry::{Spec, on_mac};
use crate::api::target;
use crate::media::tracks::TrackKind;
use crate::settings::{SUBTITLE_POSITION_RANGE, SUBTITLE_SIZE_RANGE, Settings, SubtitleStyle};
use crate::ui::player::PlayerView;

/// How far the delay commands move the subtitles.
const DELAY_STEP: f64 = 0.1;
/// How much bigger or smaller the size commands make them.
const SIZE_STEP: f32 = 0.1;
/// How far up or down the height commands move them.
const HEIGHT_STEP: f32 = 0.01;

pub const COMMANDS: &[Spec] = &[
    Spec::block(
        "block.subtitles.next_track",
        "Next Subtitles",
        "Show the file's next subtitles, and none after the last.",
        |player, _, cx| player.next_track(TrackKind::Subtitle, cx),
    )
    .tells(&[
        "block.playback.track_changed",
        "block.subtitles.on",
        "block.subtitles.off",
    ])
    .keywords(&["captions", "language", "track"])
    .setting("next_subtitle_track")
    .keys(on_mac(&["ctrl-cmd-s"], &["ctrl-alt-s"])),
    Spec::block_with(
        "block.subtitles.select_track",
        "Pick Subtitles",
        "Show one of the file's subtitles, or none.",
        TRACK,
        |args, player, _, cx| select_track(args, TrackKind::Subtitle, player, cx),
    )
    .tells(&[
        "block.playback.track_changed",
        "block.subtitles.on",
        "block.subtitles.off",
    ])
    .keywords(&["captions", "language", "track", "off"]),
    Spec::block(
        "block.subtitles.toggle",
        "Show/Hide Subtitles",
        "Hide the subtitles showing, or show the ones hidden.",
        |player, _, cx| player.turn_subtitles(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&[
        "block.subtitles.on",
        "block.subtitles.off",
        "block.playback.track_changed",
    ])
    .keywords(&["captions", "off", "on"])
    .setting("toggle_subtitles")
    .keys(&["v"])
    .state(|target, cx| {
        target::read_block(target, cx, |player, _| json!(player.shows_subtitles()))
    }),
    Spec::block(
        "block.subtitles.on",
        "Show Subtitles",
        "Show the subtitles hidden, or the file's first, unless some show already.",
        |player, _, cx| player.turn_subtitles(|on| Switch::On.turn(on), cx),
    )
    .tells(&["block.subtitles.on", "block.playback.track_changed"])
    .keywords(&["captions", "on"])
    .setting("subtitles.on"),
    Spec::block(
        "block.subtitles.off",
        "Hide Subtitles",
        "Hide the subtitles showing, unless none show.",
        |player, _, cx| player.turn_subtitles(|on| Switch::Off.turn(on), cx),
    )
    .tells(&["block.subtitles.off", "block.playback.track_changed"])
    .keywords(&["captions", "off", "none"])
    .setting("subtitles.off"),
    Spec::block(
        "block.subtitles.bigger",
        "Bigger Subtitles",
        "Make every file's subtitles bigger.",
        |player, _, cx| change_size(player, SIZE_STEP, cx),
    )
    .tells(&["app.settings.changed"])
    .keywords(&["captions", "size", "larger", "font"])
    .setting("bigger_subtitles")
    .keys(&["shift-g"]),
    Spec::block(
        "block.subtitles.smaller",
        "Smaller Subtitles",
        "Make every file's subtitles smaller.",
        |player, _, cx| change_size(player, -SIZE_STEP, cx),
    )
    .tells(&["app.settings.changed"])
    .keywords(&["captions", "size", "font"])
    .setting("smaller_subtitles")
    .keys(&["shift-f"]),
    Spec::block(
        "block.subtitles.up",
        "Move Subtitles Up",
        "Show every file's subtitles higher up.",
        |player, _, cx| change_height(player, HEIGHT_STEP, cx),
    )
    .tells(&["app.settings.changed"])
    .keywords(&["captions", "position", "raise"])
    .setting("subtitles_up")
    .keys(&["r"]),
    Spec::block(
        "block.subtitles.down",
        "Move Subtitles Down",
        "Show every file's subtitles lower down.",
        |player, _, cx| change_height(player, -HEIGHT_STEP, cx),
    )
    .tells(&["app.settings.changed"])
    .keywords(&["captions", "position", "lower"])
    .setting("subtitles_down")
    .keys(&["shift-r", "t"]),
    Spec::block(
        "block.subtitles.earlier",
        "Subtitles Earlier",
        "Show the block's subtitles a little earlier.",
        |player, _, cx| player.shift_subtitles(-DELAY_STEP, cx),
    )
    .keywords(&["captions", "delay", "sync", "offset"])
    .setting("subtitles_earlier")
    .keys(&["z"]),
    Spec::block(
        "block.subtitles.later",
        "Subtitles Later",
        "Show the block's subtitles a little later.",
        |player, _, cx| player.shift_subtitles(DELAY_STEP, cx),
    )
    .keywords(&["captions", "delay", "sync", "offset"])
    .setting("subtitles_later")
    .keys(&["x"]),
    Spec::block(
        "block.subtitles.reset_delay",
        "Reset Subtitle Delay",
        "Show the block's subtitles at their own times again.",
        |player, _, cx| player.set_subtitle_delay(0.0, cx),
    )
    .keywords(&["captions", "sync", "offset"])
    .setting("reset_subtitle_delay")
    .keys(&["shift-backspace"]),
    Spec::blocks(
        "window.subtitles.next_track",
        "Next Subtitles in Window",
        "Show the next subtitles of every file in the window, and none after the last.",
        |players, cx| players.each(cx, |player, cx| player.next_track(TrackKind::Subtitle, cx)),
    )
    .tells(&[
        "block.playback.track_changed",
        "block.subtitles.on",
        "block.subtitles.off",
    ])
    .keywords(&["every", "all", "captions", "language", "track"]),
    Spec::blocks_with(
        "window.subtitles.select_track",
        "Pick Subtitles in Window",
        "Show subtitles of every file in the window that has them, or none.",
        TRACK,
        |args, players, cx| select_tracks(args, TrackKind::Subtitle, players, cx),
    )
    .tells(&[
        "block.playback.track_changed",
        "block.subtitles.on",
        "block.subtitles.off",
    ])
    .keywords(&["every", "all", "captions", "language", "track", "off"]),
    Spec::blocks(
        "window.subtitles.toggle",
        "Show/Hide Subtitles in Window",
        "Hide the subtitles of every file in the window if each that has some shows them, else show them in all.",
        |players, cx| players.turn_subtitles(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&[
        "block.subtitles.on",
        "block.subtitles.off",
        "block.playback.track_changed",
    ])
    .keywords(&["every", "all", "captions", "off", "on"]),
    Spec::blocks(
        "window.subtitles.on",
        "Show Subtitles in Window",
        "Show the subtitles hidden, or the first, of every file in the window.",
        |players, cx| players.turn_subtitles(|on| Switch::On.turn(on), cx),
    )
    .tells(&[
        "block.subtitles.on",
        "block.playback.track_changed",
    ])
    .keywords(&["every", "all", "captions", "on"]),
    Spec::blocks(
        "window.subtitles.off",
        "Hide Subtitles in Window",
        "Hide the subtitles of every file in the window.",
        |players, cx| players.turn_subtitles(|on| Switch::Off.turn(on), cx),
    )
    .tells(&[
        "block.subtitles.off",
        "block.playback.track_changed",
    ])
    .keywords(&["every", "all", "captions", "off", "none"]),
    Spec::blocks(
        "window.subtitles.earlier",
        "Subtitles Earlier in Window",
        "Show the subtitles of every file in the window a little earlier.",
        |players, cx| players.each(cx, |player, cx| player.shift_subtitles(-DELAY_STEP, cx)),
    )
    .keywords(&["every", "all", "captions", "delay", "sync", "offset"]),
    Spec::blocks(
        "window.subtitles.later",
        "Subtitles Later in Window",
        "Show the subtitles of every file in the window a little later.",
        |players, cx| players.each(cx, |player, cx| player.shift_subtitles(DELAY_STEP, cx)),
    )
    .keywords(&["every", "all", "captions", "delay", "sync", "offset"]),
    Spec::blocks(
        "window.subtitles.reset_delay",
        "Reset Subtitle Delay in Window",
        "Show the subtitles of every file in the window at their own times again.",
        |players, cx| players.each(cx, |player, cx| player.set_subtitle_delay(0.0, cx)),
    )
    .keywords(&["every", "all", "captions", "sync", "offset"]),
    Spec::every_block(
        "app.subtitles.next_track",
        "Next Subtitles in Every Window",
        "Show the next subtitles of every file in every window, and none after the last.",
        |players, cx| players.each(cx, |player, cx| player.next_track(TrackKind::Subtitle, cx)),
    )
    .tells(&[
        "block.playback.track_changed",
        "block.subtitles.on",
        "block.subtitles.off",
    ])
    .keywords(&["every", "all", "captions", "language", "track"]),
    Spec::every_block_with(
        "app.subtitles.select_track",
        "Pick Subtitles in Every Window",
        "Show subtitles of every file in every window that has them, or none.",
        TRACK,
        |args, players, cx| select_tracks(args, TrackKind::Subtitle, players, cx),
    )
    .tells(&[
        "block.playback.track_changed",
        "block.subtitles.on",
        "block.subtitles.off",
    ])
    .keywords(&["every", "all", "captions", "language", "track", "off"]),
    Spec::every_block(
        "app.subtitles.toggle",
        "Show/Hide Subtitles in Every Window",
        "Hide the subtitles of every file in every window if each that has some shows them, else show them in all.",
        |players, cx| players.turn_subtitles(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&[
        "block.subtitles.on",
        "block.subtitles.off",
        "block.playback.track_changed",
    ])
    .keywords(&["every", "all", "captions", "off", "on"]),
    Spec::every_block(
        "app.subtitles.on",
        "Show Subtitles in Every Window",
        "Show the subtitles hidden, or the first, of every file in every window.",
        |players, cx| players.turn_subtitles(|on| Switch::On.turn(on), cx),
    )
    .tells(&[
        "block.subtitles.on",
        "block.playback.track_changed",
    ])
    .keywords(&["every", "all", "captions", "on"]),
    Spec::every_block(
        "app.subtitles.off",
        "Hide Subtitles in Every Window",
        "Hide the subtitles of every file in every window.",
        |players, cx| players.turn_subtitles(|on| Switch::Off.turn(on), cx),
    )
    .tells(&[
        "block.subtitles.off",
        "block.playback.track_changed",
    ])
    .keywords(&["every", "all", "captions", "off", "none"]),
    Spec::every_block(
        "app.subtitles.earlier",
        "Subtitles Earlier in Every Window",
        "Show the subtitles of every file in every window a little earlier.",
        |players, cx| players.each(cx, |player, cx| player.shift_subtitles(-DELAY_STEP, cx)),
    )
    .keywords(&["every", "all", "captions", "delay", "sync", "offset"]),
    Spec::every_block(
        "app.subtitles.later",
        "Subtitles Later in Every Window",
        "Show the subtitles of every file in every window a little later.",
        |players, cx| players.each(cx, |player, cx| player.shift_subtitles(DELAY_STEP, cx)),
    )
    .keywords(&["every", "all", "captions", "delay", "sync", "offset"]),
    Spec::every_block(
        "app.subtitles.reset_delay",
        "Reset Subtitle Delay in Every Window",
        "Show the subtitles of every file in every window at their own times again.",
        |players, cx| players.each(cx, |player, cx| player.set_subtitle_delay(0.0, cx)),
    )
    .keywords(&["every", "all", "captions", "sync", "offset"]),
];

/// Subtitles to show, or none.
const TRACK: &[Arg] = &[Arg::optional(
    "number",
    Kind::Integer { min: 1 },
    "Which, from 1, as the tracks menu lists them; none for no subtitles.",
)];

/// The blocks' events about their subtitles.
pub const EVENTS: &[EventKind] = &[
    EventKind::block(
        "block.subtitles.on",
        "A block's subtitles show: a subtitle track was picked where none was.",
        &[],
    ),
    EventKind::block(
        "block.subtitles.off",
        "A block's subtitles hid: no subtitle track is picked now.",
        &[],
    ),
];

/// Change how every file's subtitles look, and say so on `player`.
fn change_style(
    player: &mut PlayerView,
    cx: &mut Context<PlayerView>,
    change: impl FnOnce(&mut SubtitleStyle) -> String,
) {
    let mut note = String::new();
    Settings::update(cx, true, |settings| {
        let mut style = settings.subtitles.clamped();
        note = change(&mut style);
        settings.subtitles = style;
    });
    player.note_change(note, cx);
}

fn change_size(player: &mut PlayerView, delta: f32, cx: &mut Context<PlayerView>) {
    change_style(player, cx, |style| {
        let size = ((style.size + delta) * 20.0).round() / 20.0;
        style.size = size.clamp(*SUBTITLE_SIZE_RANGE.start(), *SUBTITLE_SIZE_RANGE.end());
        format!("Subtitle size {}%", (style.size * 100.0).round())
    });
}

fn change_height(player: &mut PlayerView, delta: f32, cx: &mut Context<PlayerView>) {
    change_style(player, cx, |style| {
        let position = ((style.position + delta) * 100.0).round() / 100.0;
        style.position = position.clamp(
            *SUBTITLE_POSITION_RANGE.start(),
            *SUBTITLE_POSITION_RANGE.end(),
        );
        format!("Subtitle height {}%", (style.position * 100.0).round())
    });
}
