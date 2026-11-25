//! `playback`: playing a block, its chapters and the files beside it in its
//! folder, and every block in a window at once.

use std::path::PathBuf;

use gpui::App;
use serde::Deserialize;
use serde_json::{Value, json};

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Scope, Spec};
use crate::api::target;
use crate::api::{Error, Run};
use crate::settings::Settings;
use crate::ui::workspace::all;

pub const COMMANDS: &[Spec] = &[
    Spec::block(
        "playback.play_pause",
        "Play/Pause",
        "Pause the block if it plays, else play it.",
        |player, _, cx| player.play_pause(cx),
    )
    .tells(&["playback.playing", "playback.paused"])
    .keywords(&["play", "pause", "resume", "stop"])
    .setting("play_pause")
    .keys(&["space", "p"]),
    Spec::block(
        "playback.play",
        "Play",
        "Play the block, unless it plays already; from the start if it played to its end.",
        |player, _, cx| {
            player.play();
            cx.notify();
        },
    )
    .tells(&["playback.playing"])
    .keywords(&["resume", "start", "unpause"]),
    Spec::block(
        "playback.pause",
        "Pause",
        "Pause the block, unless it's paused already.",
        |player, _, cx| {
            player.pause();
            cx.notify();
        },
    )
    .tells(&["playback.paused"])
    .keywords(&["stop", "hold"]),
    Spec::block_with(
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
    .tells(&[
        "playback.seeked",
        "playback.ended",
        "playback.chapter_changed",
    ])
    .keywords(&["jump", "position", "time", "go to", "scrub"]),
    Spec::block(
        "playback.seek_forward",
        "Seek Forward",
        "Skip ahead by the seek step set in Settings.",
        |player, _, cx| {
            let step = seek_step(cx);
            player.seek_by(step, cx);
        },
    )
    .tells(&[
        "playback.seeked",
        "playback.ended",
        "playback.chapter_changed",
    ])
    .keywords(&["skip", "forward", "jump"])
    .setting("seek_forward")
    .keys(&["right"]),
    Spec::block(
        "playback.seek_backward",
        "Seek Backward",
        "Skip back by the seek step set in Settings.",
        |player, _, cx| {
            let step = seek_step(cx);
            player.seek_by(-step, cx);
        },
    )
    .tells(&["playback.seeked", "playback.chapter_changed"])
    .keywords(&["skip", "back", "rewind", "jump"])
    .setting("seek_backward")
    .keys(&["left"]),
    Spec::block(
        "playback.next_frame",
        "Next Frame",
        "Pause and show the next frame.",
        |player, _, cx| player.step_frame(1.0, cx),
    )
    .tells(&["playback.paused", "playback.seeked"])
    .keywords(&["step"])
    .setting("next_frame")
    .keys(&["alt-right", ".", ">"]),
    Spec::block(
        "playback.previous_frame",
        "Previous Frame",
        "Pause and show the frame before.",
        |player, _, cx| player.step_frame(-1.0, cx),
    )
    .tells(&["playback.paused", "playback.seeked"])
    .keywords(&["step", "back"])
    .setting("previous_frame")
    .keys(&["alt-left", ",", "<"]),
    Spec::block(
        "playback.next_chapter",
        "Next Chapter",
        "Play from the start of the next chapter.",
        |player, _, cx| player.next_chapter(cx),
    )
    .tells(&["playback.seeked", "playback.chapter_changed"])
    .keywords(&["skip"])
    .setting("next_chapter")
    .keys(&["pagedown"]),
    Spec::block(
        "playback.previous_chapter",
        "Previous Chapter",
        "Play from the start of this chapter, or of the one before near its start.",
        |player, _, cx| player.previous_chapter(cx),
    )
    .tells(&["playback.seeked", "playback.chapter_changed"])
    .keywords(&["back"])
    .setting("previous_chapter")
    .keys(&["pageup"]),
    Spec::block_with(
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
    .tells(&["playback.seeked", "playback.chapter_changed"])
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
        "playback.restart",
        "Restart",
        "Play from the start.",
        |player, _, cx| player.restart(cx),
    )
    .tells(&["playback.seeked", "playback.chapter_changed"])
    .keywords(&["beginning", "rewind"])
    .setting("restart")
    .keys(&["home"]),
    Spec::block(
        "playback.previous_file",
        "Previous File",
        "Play the file before this one in its playlist, or else its folder, instead.",
        |player, _, cx| player.open_neighbor(-1, cx),
    )
    .tells(&["playback.opened", "playback.playing"])
    .keywords(&["folder", "episode", "back"])
    .setting("previous_file")
    .keys(&["secondary-left"]),
    Spec::block(
        "playback.next_file",
        "Next File",
        "Play the file after this one in its playlist, or else its folder, instead.",
        |player, _, cx| player.open_neighbor(1, cx),
    )
    .tells(&["playback.opened", "playback.playing"])
    .keywords(&["folder", "episode", "skip"])
    .setting("next_file")
    .keys(&["secondary-right"]),
    Spec::block_with(
        "playback.play_file",
        "Play File Instead",
        "Play another file in the block instead, one in its folder say, at the same volume, \
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
    .tells(&["playback.opened", "playback.playing"])
    .keywords(&["open", "folder", "episode", "switch", "replace"]),
    Spec::block(
        "playback.toggle_loop",
        "Loop",
        "Play the file again from the start at its end, or stop at its end.",
        |player, _, cx| player.toggle_loop(cx),
    )
    .keywords(&["repeat"])
    .setting("loop")
    .keys(&["secondary-shift-l"])
    .state(|target, cx| {
        target::read_block(target, cx, |player, _| json!(player.player().is_looping()))
    }),
    Spec::every_block(
        "playback.play_pause_all",
        "Play/Pause All",
        "Pause every file in every window if any plays, else play them all.",
        all::toggle,
    )
    .tells(&["playback.playing", "playback.paused"])
    .keywords(&["every", "play", "pause"])
    .setting("play_pause_all")
    .keys(&["shift-space"]),
    Spec::every_block(
        "playback.play_all",
        "Play All",
        "Play every file in every window.",
        all::play,
    )
    .tells(&["playback.playing"])
    .keywords(&["every", "resume"]),
    Spec::every_block(
        "playback.pause_all",
        "Pause All",
        "Pause every file in every window.",
        all::pause,
    )
    .tells(&["playback.paused"])
    .keywords(&["every", "stop"]),
    Spec::blocks(
        "playback.play_window",
        "Play Window",
        "Play every file in the window.",
        |workspace, _, cx| workspace.set_playing(true, cx),
    )
    .tells(&["playback.playing"])
    .keywords(&["every", "all", "resume"]),
    Spec::blocks(
        "playback.pause_window",
        "Pause Window",
        "Pause every file in the window.",
        |workspace, _, cx| workspace.set_playing(false, cx),
    )
    .tells(&["playback.paused"])
    .keywords(&["every", "all", "stop"]),
    Spec::every_block(
        "playback.restart_all",
        "Restart All",
        "Play every file in every window from the start.",
        all::restart,
    )
    .tells(&["playback.seeked", "playback.chapter_changed"])
    .keywords(&["every", "beginning", "rewind"])
    .setting("restart_all")
    .keys(&["shift-home"]),
    Spec::every_block(
        "playback.seek_forward_all",
        "Seek All Forward",
        "Skip every file in every window ahead by the seek step.",
        |cx| all::seek(1.0, cx),
    )
    .tells(&[
        "playback.seeked",
        "playback.ended",
        "playback.chapter_changed",
    ])
    .keywords(&["every", "skip", "jump"])
    .setting("seek_forward_all")
    .keys(&["shift-right"]),
    Spec::every_block(
        "playback.seek_backward_all",
        "Seek All Backward",
        "Skip every file in every window back by the seek step.",
        |cx| all::seek(-1.0, cx),
    )
    .tells(&["playback.seeked", "playback.chapter_changed"])
    .keywords(&["every", "rewind", "jump"])
    .setting("seek_backward_all")
    .keys(&["shift-left"]),
    Spec::every_block(
        "playback.next_frame_all",
        "Next Frame on All",
        "Pause every file in every window and show its next frame.",
        |cx| all::step_frame(1.0, cx),
    )
    .tells(&["playback.paused", "playback.seeked"])
    .keywords(&["every", "step"])
    .setting("next_frame_all")
    .keys(&["alt-shift-right"]),
    Spec::every_block(
        "playback.previous_frame_all",
        "Previous Frame on All",
        "Pause every file in every window and show the frame before.",
        |cx| all::step_frame(-1.0, cx),
    )
    .tells(&["playback.paused", "playback.seeked"])
    .keywords(&["every", "step", "back"])
    .setting("previous_frame_all")
    .keys(&["alt-shift-left"]),
    Spec::custom(
        "playback.sync_all",
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
    .tells(&["playback.seeked", "playback.playing", "playback.paused"])
    .keywords(&["every", "align", "same time"])
    .setting("sync_all")
    .keys(&["shift-s"]),
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
        "playback.opened",
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
        "playback.open_failed",
        "A file couldn't be opened.",
        &[EventField::new("error", "Why.")],
    )
    .causes(&["a file Plays can't play or a stream it can't reach"]),
    EventKind::block(
        "playback.failed",
        "A file stopped playing on an error.",
        &[EventField::new("error", "Why.")],
    )
    .causes(&["a file that's damaged or a stream that went"]),
    EventKind::block(
        "playback.playing",
        "A file began playing: told once it moves, not as it's set to play while what it \
         plays from isn't there yet, as just after it opens or a seek.",
        PLAYHEAD,
    )
    .causes(&["a file opening while Autoplay is on", NEXT]),
    EventKind::block("playback.paused", "A file paused.", PLAYHEAD),
    EventKind::block(
        "playback.seeked",
        "A file was sought to another time, here or on the device it's cast to: where it went.",
        PLAYHEAD,
    ),
    EventKind::block(
        "playback.ended",
        "A file played to its end, and stopped there.",
        PLAYHEAD,
    )
    .causes(&["a file playing to its end"]),
    EventKind::block(
        "playback.position",
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
        "playback.chapter_changed",
        "A file is in another chapter.",
        &[EventField::new(
            "chapter",
            "The chapter, from 1; none before the first.",
        )],
    )
    .causes(&["a file playing into its next chapter"]),
    EventKind::block(
        "playback.track_changed",
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

/// Play the block in front from the start of chapter `number`, from 1.
pub fn go_to_chapter(number: usize) -> Run {
    Run::with("playback.go_to_chapter", json!({"number": number}))
}
