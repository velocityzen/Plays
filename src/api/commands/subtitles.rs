//! `subtitles`: a tile's subtitles, and how every file's look.

use gpui::Context;

use crate::api::registry::Spec;
use crate::hotkeys::Command;
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
    .hotkey(Command::NextSubtitleTrack),
    Spec::tile(
        "subtitles.toggle",
        "Show/Hide Subtitles",
        "Hide the subtitles showing, or show the ones hidden.",
        |player, _, cx| player.toggle_subtitles(cx),
    )
    .keywords(&["captions", "off", "on"])
    .hotkey(Command::ToggleSubtitles),
    Spec::tile(
        "subtitles.bigger",
        "Bigger Subtitles",
        "Make every file's subtitles bigger.",
        |player, _, cx| change_size(player, SIZE_STEP, cx),
    )
    .keywords(&["captions", "size", "larger", "font"])
    .hotkey(Command::BiggerSubtitles),
    Spec::tile(
        "subtitles.smaller",
        "Smaller Subtitles",
        "Make every file's subtitles smaller.",
        |player, _, cx| change_size(player, -SIZE_STEP, cx),
    )
    .keywords(&["captions", "size", "font"])
    .hotkey(Command::SmallerSubtitles),
    Spec::tile(
        "subtitles.up",
        "Move Subtitles Up",
        "Show every file's subtitles higher up.",
        |player, _, cx| change_height(player, HEIGHT_STEP, cx),
    )
    .keywords(&["captions", "position", "raise"])
    .hotkey(Command::SubtitlesUp),
    Spec::tile(
        "subtitles.down",
        "Move Subtitles Down",
        "Show every file's subtitles lower down.",
        |player, _, cx| change_height(player, -HEIGHT_STEP, cx),
    )
    .keywords(&["captions", "position", "lower"])
    .hotkey(Command::SubtitlesDown),
    Spec::tile(
        "subtitles.earlier",
        "Subtitles Earlier",
        "Show the tile's subtitles a little earlier.",
        |player, _, cx| player.shift_subtitles(-DELAY_STEP, cx),
    )
    .keywords(&["captions", "delay", "sync", "offset"])
    .hotkey(Command::SubtitlesEarlier),
    Spec::tile(
        "subtitles.later",
        "Subtitles Later",
        "Show the tile's subtitles a little later.",
        |player, _, cx| player.shift_subtitles(DELAY_STEP, cx),
    )
    .keywords(&["captions", "delay", "sync", "offset"])
    .hotkey(Command::SubtitlesLater),
    Spec::tile(
        "subtitles.reset_delay",
        "Reset Subtitle Delay",
        "Show the tile's subtitles at their own times again.",
        |player, _, cx| player.set_subtitle_delay(0.0, cx),
    )
    .keywords(&["captions", "sync", "offset"])
    .hotkey(Command::ResetSubtitleDelay),
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
