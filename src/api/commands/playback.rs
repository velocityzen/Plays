//! `playback`: playing a block, its chapters, the files beside it in its
//! folder, and how fast; and every block in a window, or in every window,
//! at once.

use std::path::PathBuf;

use gpui::App;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Switch, done};
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Args, Outcome, Scope, Spec};
use crate::api::target;
use crate::api::{Error, Run};
use crate::media::tempo::{MAX_SPEED, MIN_SPEED};
use crate::settings::Settings;
use crate::ui::player::PlayerView;
use crate::ui::workspace::all::{self, Players};

pub const COMMANDS: &[Spec] = &[
    Spec::block(
        "block.playback.play_pause",
        "Play/Pause",
        "Pause the block if it plays, else play it.",
        |player, _, cx| player.play_pause(cx),
    )
    .tells(&["block.playback.playing", "block.playback.paused"])
    .keywords(&["play", "pause", "resume", "stop"])
    .setting("play_pause")
    .keys(&["space", "p"]),
    Spec::block(
        "block.playback.play",
        "Play",
        "Play the block, unless it plays already; from the start if it played to its end.",
        |player, _, cx| {
            player.play();
            cx.notify();
        },
    )
    .tells(&["block.playback.playing"])
    .keywords(&["resume", "start", "unpause"])
    .setting("playback.play"),
    Spec::block(
        "block.playback.pause",
        "Pause",
        "Pause the block, unless it's paused already.",
        |player, _, cx| {
            player.pause();
            cx.notify();
        },
    )
    .tells(&["block.playback.paused"])
    .keywords(&["stop", "hold"])
    .setting("playback.pause"),
    Spec::block_with(
        "block.playback.seek",
        "Seek To",
        "Play from a time in the file, here or on the device it's cast to.",
        TIME,
        |args, player, _, cx| {
            let Seek { time } = args.read()?;
            player.seek_to(time, cx);
            done()
        },
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.ended",
        "block.playback.chapter_changed",
    ])
    .keywords(&["jump", "position", "time", "go to", "scrub"]),
    Spec::block(
        "block.playback.seek_forward",
        "Seek Forward",
        "Skip ahead by the seek step set in Settings.",
        |player, _, cx| {
            let step = seek_step(cx);
            player.seek_by(step, cx);
        },
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.ended",
        "block.playback.chapter_changed",
    ])
    .keywords(&["skip", "forward", "jump"])
    .setting("seek_forward")
    .keys(&["right"]),
    Spec::block(
        "block.playback.seek_backward",
        "Seek Backward",
        "Skip back by the seek step set in Settings.",
        |player, _, cx| {
            let step = seek_step(cx);
            player.seek_by(-step, cx);
        },
    )
    .tells(&["block.playback.seeked", "block.playback.chapter_changed"])
    .keywords(&["skip", "back", "rewind", "jump"])
    .setting("seek_backward")
    .keys(&["left"]),
    Spec::block(
        "block.playback.next_frame",
        "Next Frame",
        "Pause and show the next frame.",
        |player, _, cx| player.step_frame(1.0, cx),
    )
    .tells(&["block.playback.paused", "block.playback.seeked"])
    .keywords(&["step"])
    .setting("next_frame")
    .keys(&["alt-right", ".", ">"]),
    Spec::block(
        "block.playback.previous_frame",
        "Previous Frame",
        "Pause and show the frame before.",
        |player, _, cx| player.step_frame(-1.0, cx),
    )
    .tells(&["block.playback.paused", "block.playback.seeked"])
    .keywords(&["step", "back"])
    .setting("previous_frame")
    .keys(&["alt-left", ",", "<"]),
    Spec::block(
        "block.playback.next_chapter",
        "Next Chapter",
        "Play from the start of the next chapter.",
        |player, _, cx| player.next_chapter(cx),
    )
    .tells(&["block.playback.seeked", "block.playback.chapter_changed"])
    .keywords(&["skip"])
    .setting("next_chapter")
    .keys(&["pagedown"]),
    Spec::block(
        "block.playback.previous_chapter",
        "Previous Chapter",
        "Play from the start of this chapter, or of the one before near its start.",
        |player, _, cx| player.previous_chapter(cx),
    )
    .tells(&["block.playback.seeked", "block.playback.chapter_changed"])
    .keywords(&["back"])
    .setting("previous_chapter")
    .keys(&["pageup"]),
    Spec::block_with(
        "block.playback.go_to_chapter",
        "Go to Chapter",
        "Play from the start of a chapter.",
        CHAPTER,
        |args, player, _, cx| {
            let Chapter { number } = args.read()?;
            if number > player.chapters().len() {
                return Err(Error::not_found("chapter", number.to_string()));
            }
            player.go_to_chapter(number - 1, cx);
            done()
        },
    )
    .tells(&["block.playback.seeked", "block.playback.chapter_changed"])
    .keywords(&["chapters", "jump", "skip"])
    // The chapter it's in, from 1.
    .state(|target, cx| {
        target::read_block(target, cx, |player, _| {
            player
                .chapter()
                .map_or(Value::Null, |index| json!(index + 1))
        })
    }),
    Spec::block(
        "block.playback.restart",
        "Restart",
        "Play from the start.",
        |player, _, cx| player.restart(cx),
    )
    .tells(&["block.playback.seeked", "block.playback.chapter_changed"])
    .keywords(&["beginning", "rewind"])
    .setting("restart")
    .keys(&["home"]),
    Spec::block(
        "block.playback.go_live",
        "Go Live",
        "Play a live stream from where it is now, if it plays behind.",
        |player, _, cx| player.go_live(cx),
    )
    .keywords(&["live", "edge", "now", "stream", "catch up"])
    .setting("go_live")
    .keys(&["end"]),
    Spec::block(
        "block.playback.previous_file",
        "Previous File",
        "Play the file before this one in its playlist, or else its folder, instead.",
        |player, _, cx| player.open_neighbor(-1, cx),
    )
    .tells(&["block.playback.opened", "block.playback.playing"])
    .keywords(&["folder", "episode", "back"])
    .setting("previous_file")
    .keys(&["secondary-left"]),
    Spec::block(
        "block.playback.next_file",
        "Next File",
        "Play the file after this one in its playlist, or else its folder, instead.",
        |player, _, cx| player.open_neighbor(1, cx),
    )
    .tells(&["block.playback.opened", "block.playback.playing"])
    .keywords(&["folder", "episode", "skip"])
    .setting("next_file")
    .keys(&["secondary-right"]),
    Spec::block_with(
        "block.playback.play_file",
        "Play File Instead",
        "Play another file in the block instead, one in its folder say, at the same volume, \
         muted and looping as this one is, and playing if it is.",
        FILE,
        |args, player, _, cx| {
            player.play_instead(file(args)?, cx);
            done()
        },
    )
    .tells(&["block.playback.opened", "block.playback.playing"])
    .keywords(&["open", "folder", "episode", "switch", "replace"]),
    Spec::block(
        "block.playback.loop.toggle",
        "Loop",
        "Play the file again from the start at its end, or stop at its end.",
        |player, _, cx| player.turn_loop(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&["block.playback.loop.on", "block.playback.loop.off"])
    .keywords(&["repeat"])
    .setting("loop")
    .keys(&["secondary-shift-l"])
    .state(|target, cx| target::read_block(target, cx, |player, _| json!(player.is_looping()))),
    Spec::block(
        "block.playback.loop.on",
        "Start Looping",
        "Play the file again from the start at its end, unless it does already.",
        |player, _, cx| player.turn_loop(|on| Switch::On.turn(on), cx),
    )
    .tells(&["block.playback.loop.on"])
    .keywords(&["repeat", "loop"])
    .setting("playback.loop.on"),
    Spec::block(
        "block.playback.loop.off",
        "Stop Looping",
        "Stop the file at its end, unless it does already.",
        |player, _, cx| player.turn_loop(|on| Switch::Off.turn(on), cx),
    )
    .tells(&["block.playback.loop.off"])
    .keywords(&["repeat", "loop", "once"])
    .setting("playback.loop.off"),
    Spec::block(
        "block.playback.faster",
        "Faster",
        "Play the block at the next speed up, its sound at the same pitch.",
        |player, _, cx| player.change_speed(true, cx),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["speed", "fast", "rate", "tempo"])
    .setting("faster")
    .keys(&["]"]),
    Spec::block(
        "block.playback.slower",
        "Slower",
        "Play the block at the next speed down, its sound at the same pitch.",
        |player, _, cx| player.change_speed(false, cx),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["speed", "slow", "rate", "tempo"])
    .setting("slower")
    .keys(&["["]),
    Spec::block(
        "block.playback.normal_speed",
        "Normal Speed",
        "Play the block at its own speed again.",
        |player, _, cx| player.set_speed(1.0, cx),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["speed", "reset", "rate", "tempo"])
    .setting("normal_speed")
    .keys(&["backspace"]),
    Spec::block_with(
        "block.playback.set_speed",
        "Speed",
        "Play the block faster or slower, its sound at the same pitch.",
        SPEED,
        |args, player, _, cx| {
            player.set_speed(speed(args)?, cx);
            done()
        },
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["fast", "slow", "rate", "tempo"])
    // How fast it plays: 1 at its own speed.
    .state(|target, cx| target::read_block(target, cx, |player, _| json!(player.speed()))),
    Spec::custom(
        "block.playback.sync_all",
        "Sync All to This Block",
        "Bring every file in every window to where the block is, playing or paused as \
         it is.",
        Scope::Block,
        |_, target, cx| {
            let (_, block) = target::block(target, cx)?;
            all::sync_to(&block, cx);
            Ok(Value::Null)
        },
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.playing",
        "block.playback.paused",
    ])
    .keywords(&["every", "align", "same time"])
    .setting("sync_all")
    .keys(&["shift-s"]),
    Spec::blocks(
        "window.playback.play_pause",
        "Play/Pause in Window",
        "Pause every file in the window if any plays, else play them all.",
        |players, cx| players.play_pause(cx),
    )
    .tells(&[
        "block.playback.playing",
        "block.playback.paused",
    ])
    .keywords(&["every", "all", "play", "pause"]),
    Spec::blocks(
        "window.playback.play",
        "Play in Window",
        "Play every file in the window.",
        |players, cx| players.set_playing(true, cx),
    )
    .tells(&["block.playback.playing"])
    .keywords(&["every", "all", "resume"])
    .setting("playback.play_window"),
    Spec::blocks(
        "window.playback.pause",
        "Pause in Window",
        "Pause every file in the window.",
        |players, cx| players.set_playing(false, cx),
    )
    .tells(&["block.playback.paused"])
    .keywords(&["every", "all", "stop"])
    .setting("playback.pause_window"),
    Spec::blocks_with(
        "window.playback.seek",
        "Seek To in Window",
        "Play every file in the window from a time in it.",
        TIME,
        seek_each,
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.ended",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "jump", "position", "time"]),
    Spec::blocks(
        "window.playback.seek_forward",
        "Seek Forward in Window",
        "Skip every file in the window ahead by the seek step.",
        |players, cx| {
            let step = seek_step(cx);
            players.each(cx, |player, cx| player.seek_by(step, cx));
        },
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.ended",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "skip", "jump"]),
    Spec::blocks(
        "window.playback.seek_backward",
        "Seek Backward in Window",
        "Skip every file in the window back by the seek step.",
        |players, cx| {
            let step = seek_step(cx);
            players.each(cx, |player, cx| player.seek_by(-step, cx));
        },
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "rewind", "jump"]),
    Spec::blocks(
        "window.playback.next_frame",
        "Next Frame in Window",
        "Pause every file in the window and show its next frame.",
        |players, cx| players.each(cx, |player, cx| player.step_frame(1.0, cx)),
    )
    .tells(&[
        "block.playback.paused",
        "block.playback.seeked",
    ])
    .keywords(&["every", "all", "step"]),
    Spec::blocks(
        "window.playback.previous_frame",
        "Previous Frame in Window",
        "Pause every file in the window and show the frame before.",
        |players, cx| players.each(cx, |player, cx| player.step_frame(-1.0, cx)),
    )
    .tells(&[
        "block.playback.paused",
        "block.playback.seeked",
    ])
    .keywords(&["every", "all", "step", "back"]),
    Spec::blocks(
        "window.playback.next_chapter",
        "Next Chapter in Window",
        "Play every file in the window from the start of its next chapter.",
        |players, cx| players.each(cx, PlayerView::next_chapter),
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "skip"]),
    Spec::blocks(
        "window.playback.previous_chapter",
        "Previous Chapter in Window",
        "Play every file in the window from the start of its chapter, or of the one before near its start.",
        |players, cx| players.each(cx, PlayerView::previous_chapter),
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "back"]),
    Spec::blocks_with(
        "window.playback.go_to_chapter",
        "Go to Chapter in Window",
        "Play every file in the window that has a chapter from its start.",
        CHAPTER,
        go_to_chapter_in_each,
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "chapters", "jump"]),
    Spec::blocks(
        "window.playback.restart",
        "Restart in Window",
        "Play every file in the window from the start.",
        |players, cx| players.each(cx, PlayerView::restart),
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "beginning", "rewind"]),
    Spec::blocks(
        "window.playback.go_live",
        "Go Live in Window",
        "Play every live stream in the window from where it is now.",
        |players, cx| players.each(cx, PlayerView::go_live),
    )
    .keywords(&["every", "all", "live", "edge", "now", "stream"]),
    Spec::blocks(
        "window.playback.previous_file",
        "Previous File in Window",
        "Play the file before each of every file in the window in its playlist, or else its folder, instead.",
        |players, cx| players.each(cx, |player, cx| player.open_neighbor(-1, cx)),
    )
    .tells(&[
        "block.playback.opened",
        "block.playback.playing",
    ])
    .keywords(&["every", "all", "folder", "episode", "back"]),
    Spec::blocks(
        "window.playback.next_file",
        "Next File in Window",
        "Play the file after each of every file in the window in its playlist, or else its folder, instead.",
        |players, cx| players.each(cx, |player, cx| player.open_neighbor(1, cx)),
    )
    .tells(&[
        "block.playback.opened",
        "block.playback.playing",
    ])
    .keywords(&["every", "all", "folder", "episode", "skip"]),
    Spec::blocks_with(
        "window.playback.play_file",
        "Play File Instead in Window",
        "Play another file instead of every file in the window, each at its volume, muted and looping as it is, and playing if it is.",
        FILE,
        play_file_in_each,
    )
    .tells(&[
        "block.playback.opened",
        "block.playback.playing",
    ])
    .keywords(&["every", "all", "open", "switch", "replace"]),
    Spec::blocks(
        "window.playback.loop.toggle",
        "Loop in Window",
        "Play every file in the window again from the start at its end, unless all do already; else stop them all at their end.",
        |players, cx| players.turn_loop(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&[
        "block.playback.loop.on",
        "block.playback.loop.off",
    ])
    .keywords(&["every", "all", "repeat"]),
    Spec::blocks(
        "window.playback.loop.on",
        "Start Looping in Window",
        "Play every file in the window again from the start at its end.",
        |players, cx| players.turn_loop(|on| Switch::On.turn(on), cx),
    )
    .tells(&["block.playback.loop.on"])
    .keywords(&["every", "all", "repeat", "loop"]),
    Spec::blocks(
        "window.playback.loop.off",
        "Stop Looping in Window",
        "Stop every file in the window at its end.",
        |players, cx| players.turn_loop(|on| Switch::Off.turn(on), cx),
    )
    .tells(&["block.playback.loop.off"])
    .keywords(&["every", "all", "repeat", "loop", "once"]),
    Spec::blocks(
        "window.playback.faster",
        "Faster in Window",
        "Play every file in the window at the next speed up from its own, its sound at the same pitch.",
        |players, cx| players.each(cx, |player, cx| player.change_speed(true, cx)),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["every", "all", "speed", "fast", "rate"]),
    Spec::blocks(
        "window.playback.slower",
        "Slower in Window",
        "Play every file in the window at the next speed down from its own, its sound at the same pitch.",
        |players, cx| players.each(cx, |player, cx| player.change_speed(false, cx)),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["every", "all", "speed", "slow", "rate"]),
    Spec::blocks(
        "window.playback.normal_speed",
        "Normal Speed in Window",
        "Play every file in the window at its own speed again.",
        |players, cx| players.each(cx, |player, cx| player.set_speed(1.0, cx)),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["every", "all", "speed", "reset", "rate"]),
    Spec::blocks_with(
        "window.playback.set_speed",
        "Speed in Window",
        "Play every file in the window faster or slower, its sound at the same pitch.",
        SPEED,
        set_speed_of_each,
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["every", "all", "fast", "slow", "rate"]),
    Spec::every_block(
        "app.playback.play_pause",
        "Play/Pause in Every Window",
        "Pause every file in every window if any plays, else play them all.",
        |players, cx| players.play_pause(cx),
    )
    .tells(&[
        "block.playback.playing",
        "block.playback.paused",
    ])
    .keywords(&["every", "all", "play", "pause"])
    .setting("play_pause_all")
    .keys(&["shift-space"]),
    Spec::every_block(
        "app.playback.play",
        "Play in Every Window",
        "Play every file in every window.",
        |players, cx| players.set_playing(true, cx),
    )
    .tells(&["block.playback.playing"])
    .keywords(&["every", "all", "resume"])
    .setting("playback.play_all"),
    Spec::every_block(
        "app.playback.pause",
        "Pause in Every Window",
        "Pause every file in every window.",
        |players, cx| players.set_playing(false, cx),
    )
    .tells(&["block.playback.paused"])
    .keywords(&["every", "all", "stop"])
    .setting("playback.pause_all"),
    Spec::every_block_with(
        "app.playback.seek",
        "Seek To in Every Window",
        "Play every file in every window from a time in it.",
        TIME,
        seek_each,
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.ended",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "jump", "position", "time"]),
    Spec::every_block(
        "app.playback.seek_forward",
        "Seek Forward in Every Window",
        "Skip every file in every window ahead by the seek step.",
        |players, cx| {
            let step = seek_step(cx);
            players.each(cx, |player, cx| player.seek_by(step, cx));
        },
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.ended",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "skip", "jump"])
    .setting("seek_forward_all")
    .keys(&["shift-right"]),
    Spec::every_block(
        "app.playback.seek_backward",
        "Seek Backward in Every Window",
        "Skip every file in every window back by the seek step.",
        |players, cx| {
            let step = seek_step(cx);
            players.each(cx, |player, cx| player.seek_by(-step, cx));
        },
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "rewind", "jump"])
    .setting("seek_backward_all")
    .keys(&["shift-left"]),
    Spec::every_block(
        "app.playback.next_frame",
        "Next Frame in Every Window",
        "Pause every file in every window and show its next frame.",
        |players, cx| players.each(cx, |player, cx| player.step_frame(1.0, cx)),
    )
    .tells(&[
        "block.playback.paused",
        "block.playback.seeked",
    ])
    .keywords(&["every", "all", "step"])
    .setting("next_frame_all")
    .keys(&["alt-shift-right"]),
    Spec::every_block(
        "app.playback.previous_frame",
        "Previous Frame in Every Window",
        "Pause every file in every window and show the frame before.",
        |players, cx| players.each(cx, |player, cx| player.step_frame(-1.0, cx)),
    )
    .tells(&[
        "block.playback.paused",
        "block.playback.seeked",
    ])
    .keywords(&["every", "all", "step", "back"])
    .setting("previous_frame_all")
    .keys(&["alt-shift-left"]),
    Spec::every_block(
        "app.playback.next_chapter",
        "Next Chapter in Every Window",
        "Play every file in every window from the start of its next chapter.",
        |players, cx| players.each(cx, PlayerView::next_chapter),
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "skip"]),
    Spec::every_block(
        "app.playback.previous_chapter",
        "Previous Chapter in Every Window",
        "Play every file in every window from the start of its chapter, or of the one before near its start.",
        |players, cx| players.each(cx, PlayerView::previous_chapter),
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "back"]),
    Spec::every_block_with(
        "app.playback.go_to_chapter",
        "Go to Chapter in Every Window",
        "Play every file in every window that has a chapter from its start.",
        CHAPTER,
        go_to_chapter_in_each,
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "chapters", "jump"]),
    Spec::every_block(
        "app.playback.restart",
        "Restart in Every Window",
        "Play every file in every window from the start.",
        |players, cx| players.each(cx, PlayerView::restart),
    )
    .tells(&[
        "block.playback.seeked",
        "block.playback.chapter_changed",
    ])
    .keywords(&["every", "all", "beginning", "rewind"])
    .setting("restart_all")
    .keys(&["shift-home"]),
    Spec::every_block(
        "app.playback.go_live",
        "Go Live in Every Window",
        "Play every live stream in every window from where it is now.",
        |players, cx| players.each(cx, PlayerView::go_live),
    )
    .keywords(&["every", "all", "live", "edge", "now", "stream"])
    .setting("go_live_all")
    .keys(&["shift-end"]),
    Spec::every_block(
        "app.playback.previous_file",
        "Previous File in Every Window",
        "Play the file before each of every file in every window in its playlist, or else its folder, instead.",
        |players, cx| players.each(cx, |player, cx| player.open_neighbor(-1, cx)),
    )
    .tells(&[
        "block.playback.opened",
        "block.playback.playing",
    ])
    .keywords(&["every", "all", "folder", "episode", "back"]),
    Spec::every_block(
        "app.playback.next_file",
        "Next File in Every Window",
        "Play the file after each of every file in every window in its playlist, or else its folder, instead.",
        |players, cx| players.each(cx, |player, cx| player.open_neighbor(1, cx)),
    )
    .tells(&[
        "block.playback.opened",
        "block.playback.playing",
    ])
    .keywords(&["every", "all", "folder", "episode", "skip"]),
    Spec::every_block_with(
        "app.playback.play_file",
        "Play File Instead in Every Window",
        "Play another file instead of every file in every window, each at its volume, muted and looping as it is, and playing if it is.",
        FILE,
        play_file_in_each,
    )
    .tells(&[
        "block.playback.opened",
        "block.playback.playing",
    ])
    .keywords(&["every", "all", "open", "switch", "replace"]),
    Spec::every_block(
        "app.playback.loop.toggle",
        "Loop in Every Window",
        "Play every file in every window again from the start at its end, unless all do already; else stop them all at their end.",
        |players, cx| players.turn_loop(|on| Switch::Toggle.turn(on), cx),
    )
    .tells(&[
        "block.playback.loop.on",
        "block.playback.loop.off",
    ])
    .keywords(&["every", "all", "repeat"]),
    Spec::every_block(
        "app.playback.loop.on",
        "Start Looping in Every Window",
        "Play every file in every window again from the start at its end.",
        |players, cx| players.turn_loop(|on| Switch::On.turn(on), cx),
    )
    .tells(&["block.playback.loop.on"])
    .keywords(&["every", "all", "repeat", "loop"]),
    Spec::every_block(
        "app.playback.loop.off",
        "Stop Looping in Every Window",
        "Stop every file in every window at its end.",
        |players, cx| players.turn_loop(|on| Switch::Off.turn(on), cx),
    )
    .tells(&["block.playback.loop.off"])
    .keywords(&["every", "all", "repeat", "loop", "once"]),
    Spec::every_block(
        "app.playback.faster",
        "Faster in Every Window",
        "Play every file in every window at the next speed up from its own, its sound at the same pitch.",
        |players, cx| players.each(cx, |player, cx| player.change_speed(true, cx)),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["every", "all", "speed", "fast", "rate"]),
    Spec::every_block(
        "app.playback.slower",
        "Slower in Every Window",
        "Play every file in every window at the next speed down from its own, its sound at the same pitch.",
        |players, cx| players.each(cx, |player, cx| player.change_speed(false, cx)),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["every", "all", "speed", "slow", "rate"]),
    Spec::every_block(
        "app.playback.normal_speed",
        "Normal Speed in Every Window",
        "Play every file in every window at its own speed again.",
        |players, cx| players.each(cx, |player, cx| player.set_speed(1.0, cx)),
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["every", "all", "speed", "reset", "rate"]),
    Spec::every_block_with(
        "app.playback.set_speed",
        "Speed in Every Window",
        "Play every file in every window faster or slower, its sound at the same pitch.",
        SPEED,
        set_speed_of_each,
    )
    .tells(&["block.playback.speed_changed"])
    .keywords(&["every", "all", "fast", "slow", "rate"]),
];

/// What opens a file, and plays it, other than a command: a playlist
/// going on.
const NEXT: &str = "a playlist going on to its next file, once one plays to its end";

/// Where a file is, as the events of how it plays say.
const PLAYHEAD: &[EventField] = &[
    EventField::new(
        "position",
        "How far through the file it is, from 0 to 1; none while its duration isn't known, \
         as a live stream's.",
    ),
    EventField::new("time", "Where it is, in seconds."),
    EventField::new(
        "frame",
        "The number of the frame showing then, from 0; none for sound alone.",
    ),
];

/// How files play: the blocks' events about them.
pub const EVENTS: &[EventKind] = &[
    EventKind::block(
        "block.playback.loop.on",
        "A block's file is set to play again from the start at its end.",
        &[],
    ),
    EventKind::block(
        "block.playback.loop.off",
        "A block's file is set to stop at its end.",
        &[],
    ),
    EventKind::block(
        "block.playback.opened",
        "A file opened in a block, paused at its start.",
        &[
            EventField::new(
                "title",
                "What its header shows without a label: the file's name, or the stream's.",
            ),
            EventField::new("file", "Its path; none for a stream."),
            EventField::new("url", "The stream's URL; none for a file."),
            EventField::new(
                "duration",
                "How long it plays, in seconds; none for a live stream.",
            ),
        ],
    )
    .causes(&[super::DROPPED, NEXT]),
    EventKind::block(
        "block.playback.open_failed",
        "A file couldn't be opened.",
        &[EventField::new("error", "Why.")],
    )
    .causes(&["a file Plays can't play or a stream it can't reach"]),
    EventKind::block(
        "block.playback.failed",
        "A file stopped playing on an error.",
        &[EventField::new("error", "Why.")],
    )
    .causes(&["a file that's damaged or a stream that went"]),
    EventKind::block(
        "block.playback.playing",
        "A file began playing: told once it moves, not as it's set to play while what it \
         plays from isn't there yet, as just after it opens or a seek.",
        PLAYHEAD,
    )
    .causes(&["a file opening while Autoplay is on", NEXT]),
    EventKind::block("block.playback.paused", "A file paused.", PLAYHEAD),
    EventKind::block(
        "block.playback.seeked",
        "A file was sought to another time, here or on the device it's cast to: where it went.",
        PLAYHEAD,
    ),
    EventKind::block(
        "block.playback.ended",
        "A file played to its end, and stopped there.",
        PLAYHEAD,
    )
    .causes(&["a file playing to its end"]),
    EventKind::block(
        "block.playback.position",
        "Where a file is as it plays: told only to those who ask for positions, at most once \
         a second, and only once it has moved.",
        &[
            EventField::new(
                "position",
                "How far through the file it is, from 0 to 1; none while its duration isn't \
                 known, as a live stream's.",
            ),
            EventField::new("time", "Where it is, in seconds."),
            EventField::new(
                "frame",
                "The number of the frame showing then, from 0; none for sound alone.",
            ),
            EventField::new(
                "duration",
                "How long it plays, in seconds; none for a live stream.",
            ),
        ],
    )
    .causes(&["a file playing"]),
    EventKind::block(
        "block.playback.speed_changed",
        "A block's file plays faster or slower.",
        &[EventField::new(
            "speed",
            "How fast it plays: 1 at its own speed, as `block.playback.set_speed` takes it.",
        )],
    )
    .causes(&["a cast starting, which plays at its own speed"]),
    EventKind::block(
        "block.playback.chapter_changed",
        "A file is in another chapter.",
        &[EventField::new(
            "chapter",
            "The chapter, from 1; none before the first.",
        )],
    )
    .causes(&["a file playing into its next chapter"]),
    EventKind::block(
        "block.playback.track_changed",
        "Another of a file's tracks plays, or none of its kind.",
        &[
            EventField::new("kind", "The track's: video, audio or subtitles."),
            EventField::new(
                "track",
                "The one playing, from 1 among its kind's, as the tracks menu lists them; \
                 none for none.",
            ),
        ],
    ),
];

/// A time to play from.
const TIME: &[Arg] = &[Arg::required(
    "time",
    Kind::Number {
        min: 0.0,
        max: None,
    },
    "Seconds from the start; past the end is the end.",
)];

/// How fast to play.
const SPEED: &[Arg] = &[Arg::required(
    "speed",
    Kind::Number {
        min: MIN_SPEED,
        max: Some(MAX_SPEED),
    },
    "How fast: 1 at its own speed, 2 twice as fast, 0.5 half.",
)];

/// A chapter to play from.
const CHAPTER: &[Arg] = &[Arg::required(
    "number",
    Kind::Integer { min: 1 },
    "Which chapter, from 1.",
)];

/// A file to play instead.
const FILE: &[Arg] = &[Arg::required("path", Kind::Text, "The file's full path.")];

#[derive(Deserialize)]
struct Speed {
    speed: f64,
}

/// The speed `args` say.
fn speed(args: Args) -> Result<f64, Error> {
    let Speed { speed } = args.read()?;
    Ok(speed)
}

/// Play each as fast as `args` say.
fn set_speed_of_each(args: Args, players: &Players, cx: &mut App) -> Outcome {
    let speed = speed(args)?;
    players.each(cx, |player, cx| player.set_speed(speed, cx));
    done()
}

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

/// The full path of the file `args` name.
fn file(args: Args) -> Result<PathBuf, Error> {
    let PlayFile { path } = args.read()?;
    if !path.is_absolute() {
        let why = format!("{}: give a file's full path", path.display());
        return Err(Error::bad_args(args.command, why));
    }
    Ok(path)
}

/// Play each from the time `args` say.
fn seek_each(args: Args, players: &Players, cx: &mut App) -> Outcome {
    let Seek { time } = args.read()?;
    players.each(cx, |player, cx| player.seek_to(time, cx));
    done()
}

/// Play each that has the chapter `args` number from its start.
fn go_to_chapter_in_each(args: Args, players: &Players, cx: &mut App) -> Outcome {
    let Chapter { number } = args.read()?;
    if players.go_to_chapter(number - 1, cx) {
        done()
    } else {
        Err(Error::not_found("chapter", number.to_string()))
    }
}

/// Play the file `args` name instead of each.
fn play_file_in_each(args: Args, players: &Players, cx: &mut App) -> Outcome {
    let path = file(args)?;
    players.each(cx, |player, cx| player.play_instead(path.clone(), cx));
    done()
}

/// How far the seek commands skip.
fn seek_step(cx: &App) -> f64 {
    f64::from(cx.global::<Settings>().seek_step_seconds)
}

/// Play the block in front at `speed`.
pub fn set_speed(speed: f64) -> Run {
    Run::with("block.playback.set_speed", json!({"speed": speed}))
}

/// Play the block in front from the start of chapter `number`, from 1.
pub fn go_to_chapter(number: usize) -> Run {
    Run::with("block.playback.go_to_chapter", json!({"number": number}))
}
