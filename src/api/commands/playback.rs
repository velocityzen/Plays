//! `playback`: playing a tile, its chapters and the files beside it in its
//! folder, and every tile in a window at once.

use std::path::PathBuf;

use gpui::App;
use serde::Deserialize;
use serde_json::{Value, json};

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::registry::Spec;
use crate::api::target;
use crate::api::{Error, Run};
use crate::settings::Settings;

pub const COMMANDS: &[Spec] = &[
    Spec::tile(
        "playback.play_pause",
        "Play/Pause",
        "Pause the tile if it plays, else play it.",
        |player, _, cx| player.play_pause(cx),
    )
    .keywords(&["play", "pause", "resume", "stop"])
    .setting("play_pause")
    .keys(&["space", "p"]),
    Spec::tile_with(
        "playback.seek",
        "Seek To",
        "Play from a time in the file, here or on the device it's cast to.",
        &[Arg::required(
            "time",
            Kind::Number {
                min: 0.0,
                max: None,
            },
            "Seconds from the start; past the end is the end.",
        )],
        |args, player, _, cx| {
            let Seek { time } = args.read()?;
            player.seek_to(time, cx);
            done()
        },
    )
    .keywords(&["jump", "position", "time", "go to", "scrub"]),
    Spec::tile(
        "playback.seek_forward",
        "Seek Forward",
        "Skip ahead by the seek step set in Settings.",
        |player, _, cx| {
            let step = seek_step(cx);
            player.seek_by(step, cx);
        },
    )
    .keywords(&["skip", "forward", "jump"])
    .setting("seek_forward")
    .keys(&["right"]),
    Spec::tile(
        "playback.seek_backward",
        "Seek Backward",
        "Skip back by the seek step set in Settings.",
        |player, _, cx| {
            let step = seek_step(cx);
            player.seek_by(-step, cx);
        },
    )
    .keywords(&["skip", "back", "rewind", "jump"])
    .setting("seek_backward")
    .keys(&["left"]),
    Spec::tile(
        "playback.next_frame",
        "Next Frame",
        "Pause and show the next frame.",
        |player, _, cx| player.step_frame(1.0, cx),
    )
    .keywords(&["step"])
    .setting("next_frame")
    .keys(&["alt-right", ".", ">"]),
    Spec::tile(
        "playback.previous_frame",
        "Previous Frame",
        "Pause and show the frame before.",
        |player, _, cx| player.step_frame(-1.0, cx),
    )
    .keywords(&["step", "back"])
    .setting("previous_frame")
    .keys(&["alt-left", ",", "<"]),
    Spec::tile(
        "playback.next_chapter",
        "Next Chapter",
        "Play from the start of the next chapter.",
        |player, _, cx| player.next_chapter(cx),
    )
    .keywords(&["skip"])
    .setting("next_chapter")
    .keys(&["pagedown"]),
    Spec::tile(
        "playback.previous_chapter",
        "Previous Chapter",
        "Play from the start of this chapter, or of the one before near its start.",
        |player, _, cx| player.previous_chapter(cx),
    )
    .keywords(&["back"])
    .setting("previous_chapter")
    .keys(&["pageup"]),
    Spec::tile_with(
        "playback.go_to_chapter",
        "Go to Chapter",
        "Play from the start of a chapter.",
        &[Arg::required(
            "number",
            Kind::Integer { min: 1 },
            "Which chapter, from 1.",
        )],
        |args, player, _, cx| {
            let Chapter { number } = args.read()?;
            if number > player.chapters().len() {
                return Err(Error::not_found("chapter", number.to_string()));
            }
            player.go_to_chapter(number - 1, cx);
            done()
        },
    )
    .keywords(&["chapters", "jump", "skip"])
    // The chapter it's in, from 1.
    .state(|target, cx| {
        target::read_tile(target, cx, |player, _| {
            player
                .chapter()
                .map_or(Value::Null, |index| json!(index + 1))
        })
    }),
    Spec::tile(
        "playback.restart",
        "Restart",
        "Play from the start.",
        |player, _, cx| player.restart(cx),
    )
    .keywords(&["beginning", "rewind"])
    .setting("restart")
    .keys(&["home"]),
    Spec::tile(
        "playback.previous_file",
        "Previous File",
        "Play the file before this one in its playlist, or else its folder, instead.",
        |player, _, cx| player.open_neighbor(-1, cx),
    )
    .keywords(&["folder", "episode", "back"])
    .setting("previous_file")
    .keys(&["secondary-left"]),
    Spec::tile(
        "playback.next_file",
        "Next File",
        "Play the file after this one in its playlist, or else its folder, instead.",
        |player, _, cx| player.open_neighbor(1, cx),
    )
    .keywords(&["folder", "episode", "skip"])
    .setting("next_file")
    .keys(&["secondary-right"]),
    Spec::tile_with(
        "playback.play_file",
        "Play File Instead",
        "Play another file in the tile instead, one in its folder say, at the same volume, \
         muted and looping as this one is, and playing if it is.",
        &[Arg::required("path", Kind::Text, "The file's full path.")],
        |args, player, _, cx| {
            let PlayFile { path } = args.read()?;
            if !path.is_absolute() {
                let why = format!("{}: give a file's full path", path.display());
                return Err(Error::bad_args(args.command, why));
            }
            player.play_instead(path, cx);
            done()
        },
    )
    .keywords(&["open", "folder", "episode", "switch", "replace"]),
    Spec::tile(
        "playback.toggle_loop",
        "Loop",
        "Play the file again from the start at its end, or stop at its end.",
        |player, _, cx| player.toggle_loop(cx),
    )
    .keywords(&["repeat"])
    .setting("loop")
    .keys(&["secondary-shift-l"])
    .state(|target, cx| {
        target::read_tile(target, cx, |player, _| json!(player.player().is_looping()))
    }),
    Spec::tiles(
        "playback.play_pause_all",
        "Play/Pause All",
        "Pause every file in the window if any plays, else play them all.",
        |workspace, _, cx| workspace.toggle_all(cx),
    )
    .keywords(&["every", "play", "pause"])
    .setting("play_pause_all")
    .keys(&["shift-space"]),
    Spec::tiles(
        "playback.play_all",
        "Play All",
        "Play every file in the window.",
        |workspace, _, cx| workspace.play_all(cx),
    )
    .keywords(&["every", "resume"]),
    Spec::tiles(
        "playback.pause_all",
        "Pause All",
        "Pause every file in the window.",
        |workspace, _, cx| workspace.pause_all(cx),
    )
    .keywords(&["every", "stop"]),
    Spec::tiles(
        "playback.restart_all",
        "Restart All",
        "Play every file in the window from the start.",
        |workspace, _, cx| workspace.restart_all(cx),
    )
    .keywords(&["every", "beginning", "rewind"])
    .setting("restart_all")
    .keys(&["shift-home"]),
    Spec::tiles(
        "playback.seek_forward_all",
        "Seek All Forward",
        "Skip every file in the window ahead by the seek step.",
        |workspace, _, cx| workspace.seek_all(1.0, cx),
    )
    .keywords(&["every", "skip", "jump"])
    .setting("seek_forward_all")
    .keys(&["shift-right"]),
    Spec::tiles(
        "playback.seek_backward_all",
        "Seek All Backward",
        "Skip every file in the window back by the seek step.",
        |workspace, _, cx| workspace.seek_all(-1.0, cx),
    )
    .keywords(&["every", "rewind", "jump"])
    .setting("seek_backward_all")
    .keys(&["shift-left"]),
    Spec::tiles(
        "playback.next_frame_all",
        "Next Frame on All",
        "Pause every file in the window and show its next frame.",
        |workspace, _, cx| workspace.step_frame_all(1.0, cx),
    )
    .keywords(&["every", "step"])
    .setting("next_frame_all")
    .keys(&["alt-shift-right"]),
    Spec::tiles(
        "playback.previous_frame_all",
        "Previous Frame on All",
        "Pause every file in the window and show the frame before.",
        |workspace, _, cx| workspace.step_frame_all(-1.0, cx),
    )
    .keywords(&["every", "step", "back"])
    .setting("previous_frame_all")
    .keys(&["alt-shift-left"]),
    Spec::tiles(
        "playback.sync_all",
        "Sync All to This Tile",
        "Bring every file in the window to where the tile keys go to is, playing or \
         paused as it is.",
        |workspace, _, cx| workspace.sync_all(cx),
    )
    .keywords(&["every", "align", "same time"])
    .setting("sync_all")
    .keys(&["shift-s"]),
];

#[derive(Deserialize)]
struct Chapter {
    number: usize,
}

#[derive(Deserialize)]
struct Seek {
    time: f64,
}

#[derive(Deserialize)]
struct PlayFile {
    path: PathBuf,
}

/// How far the seek commands skip.
fn seek_step(cx: &App) -> f64 {
    f64::from(cx.global::<Settings>().seek_step_seconds)
}

/// Play the tile in front from the start of chapter `number`, from 1.
pub fn go_to_chapter(number: usize) -> Run {
    Run::with("playback.go_to_chapter", json!({"number": number}))
}
