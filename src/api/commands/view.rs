//! `view`: which panels show over the video in every tile, each turned
//! off to keep the picture clear of it, as Settings → Appearance has them.

use gpui::App;
use serde_json::json;

use crate::api::registry::Spec;
use crate::settings::{OverVideo, Settings};

pub const COMMANDS: &[Spec] = &[
    Spec::app(
        "view.toggle_controls",
        "Show Controls",
        "Show the seek bar and buttons at the bottom of each tile, or never show them.",
        |cx| toggle(cx, |shown| &mut shown.controls),
    )
    .keywords(&["hide", "player controls", "seek bar", "bottom bar"])
    .state(|_, cx| Ok(json!(shown(cx).controls))),
    Spec::app(
        "view.toggle_top_bar",
        "Show Top Bar",
        "Show the file's name and buttons at the top of each tile, or never show them.",
        |cx| toggle(cx, |shown| &mut shown.top_bar),
    )
    .keywords(&["hide", "header", "title", "buttons"])
    .state(|_, cx| Ok(json!(shown(cx).top_bar))),
    Spec::app(
        "view.toggle_track_switcher",
        "Show Track Switcher",
        "Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, \
         or switch without it.",
        |cx| toggle(cx, |shown| &mut shown.track_switcher),
    )
    .keywords(&["hide", "reel", "streams", "audio track", "subtitle track"])
    .state(|_, cx| Ok(json!(shown(cx).track_switcher))),
    Spec::app(
        "view.toggle_notes",
        "Show Notes",
        "Show notes on what a shortcut changed, like the volume, or not. Notes on what's \
         wrong still show.",
        |cx| toggle(cx, |shown| &mut shown.notes),
    )
    .keywords(&["hide", "osd", "on-screen display", "volume", "messages"])
    .state(|_, cx| Ok(json!(shown(cx).notes))),
];

/// What shows over the video now.
fn shown(cx: &App) -> OverVideo {
    cx.global::<Settings>().over_video
}

/// Turn the panel `panel` picks on, or off, and keep it so.
fn toggle(cx: &mut App, panel: fn(&mut OverVideo) -> &mut bool) {
    Settings::update(cx, true, |settings| {
        let on = panel(&mut settings.over_video);
        *on = !*on;
    });
}
