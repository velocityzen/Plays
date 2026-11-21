//! `subtitles`: a tile's subtitles, and how every file's look.

use gpui::Context;

use super::select_track;
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Spec, on_mac};
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
    Spec::tile(
        "subtitles.next_track",
        "Next Subtitles",
        "Show the file's next subtitles, and none after the last.",
        |player, _, cx| player.next_track(TrackKind::Subtitle, cx),
    )
    .keywords(&["captions", "language", "track"])
    .setting("next_subtitle_track")
    .keys(on_mac(&["ctrl-cmd-s"], &["ctrl-alt-s"])),
    Spec::tile_with(
        "subtitles.select_track",
        "Pick Subtitles",
        "Show one of the file's subtitles, or none.",
        &[Arg::optional(
            "number",
            Kind::Integer { min: 1 },
            "Which, from 1, as the tracks menu lists them; none for no subtitles.",
        )],
        |args, player, _, cx| select_track(args, TrackKind::Subtitle, player, cx),
    )
    .keywords(&["captions", "language", "track", "off"]),
    Spec::tile(
        "subtitles.toggle",
        "Show/Hide Subtitles",
        "Hide the subtitles showing, or show the ones hidden.",
        |player, _, cx| player.toggle_subtitles(cx),
    )
    .keywords(&["captions", "off", "on"])
    .setting("toggle_subtitles")
    .keys(&["v"]),
    Spec::tile(
        "subtitles.bigger",
        "Bigger Subtitles",
        "Make every file's subtitles bigger.",
        |player, _, cx| change_size(player, SIZE_STEP, cx),
    )
    .keywords(&["captions", "size", "larger", "font"])
    .setting("bigger_subtitles")
    .keys(&["shift-g"]),
    Spec::tile(
        "subtitles.smaller",
        "Smaller Subtitles",
        "Make every file's subtitles smaller.",
        |player, _, cx| change_size(player, -SIZE_STEP, cx),
    )
    .keywords(&["captions", "size", "font"])
    .setting("smaller_subtitles")
    .keys(&["shift-f"]),
    Spec::tile(
        "subtitles.up",
        "Move Subtitles Up",
        "Show every file's subtitles higher up.",
        |player, _, cx| change_height(player, HEIGHT_STEP, cx),
    )
    .keywords(&["captions", "position", "raise"])
    .setting("subtitles_up")
    .keys(&["r"]),
    Spec::tile(
        "subtitles.down",
        "Move Subtitles Down",
        "Show every file's subtitles lower down.",
        |player, _, cx| change_height(player, -HEIGHT_STEP, cx),
    )
    .keywords(&["captions", "position", "lower"])
    .setting("subtitles_down")
    .keys(&["shift-r", "t"]),
    Spec::tile(
        "subtitles.earlier",
        "Subtitles Earlier",
        "Show the tile's subtitles a little earlier.",
        |player, _, cx| player.shift_subtitles(-DELAY_STEP, cx),
    )
    .keywords(&["captions", "delay", "sync", "offset"])
    .setting("subtitles_earlier")
    .keys(&["z"]),
    Spec::tile(
        "subtitles.later",
        "Subtitles Later",
        "Show the tile's subtitles a little later.",
        |player, _, cx| player.shift_subtitles(DELAY_STEP, cx),
    )
    .keywords(&["captions", "delay", "sync", "offset"])
    .setting("subtitles_later")
    .keys(&["x"]),
    Spec::tile(
        "subtitles.reset_delay",
        "Reset Subtitle Delay",
        "Show the tile's subtitles at their own times again.",
        |player, _, cx| player.set_subtitle_delay(0.0, cx),
    )
    .keywords(&["captions", "sync", "offset"])
    .setting("reset_subtitle_delay")
    .keys(&["shift-backspace"]),
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
    player.note(note, cx);
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
