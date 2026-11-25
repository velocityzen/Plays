//! `block`: the blocks in a window: closing them, going from one to another,
//! moving them about the grid, and what shows over or apart from one.

use gpui::{App, Context, Entity};
use serde::Deserialize;
use serde_json::{Value, json};

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Args, Outcome, Reach, Scope, Spec, on_mac};
use crate::api::target::{self, Target};
use crate::api::{Error, Run};
use crate::settings::{OverVideo, Panel};
use crate::ui::player::PlayerView;
use crate::ui::workspace::{Toward, block_label_taken};

pub const COMMANDS: &[Spec] = &[
    Spec::query(
        "block.ls",
        "List Blocks",
        "List the blocks in the window, in the grid's order.",
        Scope::Window,
        "[{id, label, title, file, url, playing, opened, error, number, front}]: each \
         block's id and label (or null), its file's name, the file's path or the stream's \
         URL (without a user, password or query; the other null), whether it plays, \
         whether its file has opened, why it couldn't open or play on (or null: still \
         opening, if it hasn't opened), its place from 1, and whether keys go to it.",
        |_, target, cx| {
            let window = target::window(target, cx)?;
            Ok(Value::from(target::read(window, cx)?.blocks_listed(cx)))
        },
    )
    .keywords(&["list", "ls", "blocks", "files"]),
    Spec::query(
        "block.inspect",
        "Inspect Block",
        "Say all about the block: what it plays, where it is, and how it's set to play.",
        Scope::Block,
        "{as block.ls, window, window_title, position, duration, frame_rate, ended, \
         volume, muted, looping, tracks: {video, audio, subtitles: [{number, label, \
         language, title, codec, format, default, forced, hearing_impaired, file}]}, \
         picked: {video, audio, subtitles}, delays: {audio, subtitles}, picture: {turns, \
         mirrored, aspect_ratio, crop}, chapter, chapters: [{number, name, start, end}], \
         casting: {id, kind, name} or null, output: {id, name} or null}: its window's id \
         and title, times in seconds (duration null for a live stream), frames a second \
         (null for sound alone), volume from 0 to 100, tracks and chapters numbered from \
         1, the tracks picked and the chapter by number or null, delays in seconds, \
         quarter turns clockwise, and the output it plays through of its own.",
        |_, target, cx| {
            let (window, block) = target::block(target, cx)?;
            target::read(window, cx)?
                .inspect_block(&block, cx)
                .ok_or_else(closed)
        },
    )
    .keywords(&["info", "details", "describe", "tracks", "position"]),
    Spec::custom(
        "block.focus",
        "Go to Block",
        "Put the block in front, keys going to it, and bring its window to the front.",
        Scope::Block,
        focus,
    )
    .tells(&["window.active_changed", "block.active_changed"])
    .keywords(&["switch", "select", "show", "activate"])
    // The block in front is in front already.
    .needs_target(),
    Spec::custom(
        "block.close",
        "Close File",
        "Close the block's file. The window stays, empty after the last.",
        Scope::Block,
        close,
    )
    .tells(&[
        "block.closed",
        "window.layout_changed",
        "block.active_changed",
    ])
    .keywords(&["remove", "stop"])
    .keys(&["secondary-w"])
    // Typing a URL over the blocks too, but where a key that works anywhere
    // is this one's as well, as Close Window's ⌘W is, that one wins.
    .reach(Reach::Files),
    Spec::blocks(
        "block.close_all",
        "Close All Files",
        "Close every file in the window. The window stays, empty.",
        |workspace, _, cx| workspace.close_all(cx),
    )
    .tells(&[
        "block.closed",
        "window.layout_changed",
        "block.active_changed",
    ])
    .keywords(&["clear", "remove", "stop"])
    .keys(&["secondary-shift-w"])
    .reach(Reach::Files),
    Spec::custom(
        "block.close_others",
        "Close Other Files",
        "Close every file in the window but the block's.",
        Scope::Block,
        close_others,
    )
    .tells(&["block.closed", "window.layout_changed"])
    .keywords(&["close all", "others", "keep", "remove"]),
    Spec::window(
        "block.focus.left",
        "Focus Block to the Left",
        "Send keys to the block left of the one they go to.",
        |workspace, _, cx| workspace.focus_block(Toward::Left, cx),
    )
    .tells(&["block.active_changed"])
    .setting("focus_left")
    .keys(&["secondary-alt-left"]),
    Spec::window(
        "block.focus.right",
        "Focus Block to the Right",
        "Send keys to the block right of the one they go to.",
        |workspace, _, cx| workspace.focus_block(Toward::Right, cx),
    )
    .tells(&["block.active_changed"])
    .setting("focus_right")
    .keys(&["secondary-alt-right"]),
    Spec::window(
        "block.focus.up",
        "Focus Block Above",
        "Send keys to the block above the one they go to.",
        |workspace, _, cx| workspace.focus_block(Toward::Up, cx),
    )
    .tells(&["block.active_changed"])
    .setting("focus_up")
    .keys(&["secondary-alt-up"]),
    Spec::window(
        "block.focus.down",
        "Focus Block Below",
        "Send keys to the block below the one they go to.",
        |workspace, _, cx| workspace.focus_block(Toward::Down, cx),
    )
    .tells(&["block.active_changed"])
    .setting("focus_down")
    .keys(&["secondary-alt-down"]),
    Spec::window(
        "block.focus.previous",
        "Focus Previous Block",
        "Send keys to the block before the one they go to, the last before the first.",
        |workspace, _, cx| workspace.focus_block(Toward::Previous, cx),
    )
    .tells(&["block.active_changed"])
    .setting("focus_previous_block")
    .keys(&["secondary-["]),
    Spec::window(
        "block.focus.next",
        "Focus Next Block",
        "Send keys to the block after the one they go to, the first after the last.",
        |workspace, _, cx| workspace.focus_block(Toward::Next, cx),
    )
    .tells(&["block.active_changed"])
    .setting("focus_next_block")
    .keys(&["secondary-]"]),
    Spec::window(
        "block.back",
        "Go Back One Block",
        "Send keys to the block clicked or gone to before the one they go to; again, the \
         one before that, and so on, until something else is done.",
        |workspace, _, cx| workspace.back_block(cx),
    )
    .tells(&["block.active_changed"])
    .keywords(&["history", "previous", "last", "return", "focus"]),
    Spec::window(
        "block.recent",
        "Go to Recent Block",
        "Send keys to the block clicked or gone to last but the one they go to; again, \
         back to that one.",
        |workspace, _, cx| workspace.recent_block(cx),
    )
    .tells(&["block.active_changed"])
    .keywords(&["history", "last", "toggle", "alternate", "switch", "focus"]),
    Spec::window(
        "block.move.left",
        "Move Block Left",
        "Swap the block keys go to with the one to its left.",
        |workspace, _, cx| workspace.move_front_block(Toward::Left, cx),
    )
    .tells(&["window.layout_changed"])
    .setting("move_block_left")
    .keys(&["secondary-alt-shift-left"]),
    Spec::window(
        "block.move.right",
        "Move Block Right",
        "Swap the block keys go to with the one to its right.",
        |workspace, _, cx| workspace.move_front_block(Toward::Right, cx),
    )
    .tells(&["window.layout_changed"])
    .setting("move_block_right")
    .keys(&["secondary-alt-shift-right"]),
    Spec::window(
        "block.move.up",
        "Move Block Up",
        "Swap the block keys go to with the one above it.",
        |workspace, _, cx| workspace.move_front_block(Toward::Up, cx),
    )
    .tells(&["window.layout_changed"])
    .setting("move_block_up")
    .keys(&["secondary-alt-shift-up"]),
    Spec::window(
        "block.move.down",
        "Move Block Down",
        "Swap the block keys go to with the one below it.",
        |workspace, _, cx| workspace.move_front_block(Toward::Down, cx),
    )
    .tells(&["window.layout_changed"])
    .setting("move_block_down")
    .keys(&["secondary-alt-shift-down"]),
    Spec::custom(
        "block.zoom",
        "Maximize Block",
        "Fill the window with the block, or show every block again; a window's only \
         block fills the screen instead.",
        Scope::Block,
        zoom,
    )
    .tells(&["window.full_screen_changed", "window.layout_changed"])
    .keywords(&["expand", "enlarge", "full", "restore"])
    .keys(&["secondary-shift-enter"]),
    Spec::custom_with(
        "block.rename",
        "Rename Block…",
        "Label the block: its header shows the label in place of the file's name, and it \
         can be named by it. A blank one takes the label away; with none given, the \
         window asks for one.",
        Scope::Block,
        &[Arg::optional(
            "label",
            Kind::Text,
            "What to call it; no two blocks are called the same, in whichever windows.",
        )],
        rename,
    )
    .tells(&["block.label_changed"])
    .keywords(&["name", "label", "title"]),
    Spec::custom_with(
        "block.move",
        "Move Block To",
        "Put the block in another place in its window's grid, the blocks between moving \
         over, as dragging it onto another does.",
        Scope::Block,
        &[Arg::required(
            "position",
            Kind::Integer { min: 1 },
            "Where, from 1, the first block's place.",
        )],
        put,
    )
    .tells(&["window.layout_changed"])
    .keywords(&["reorder", "place", "drag", "swap"])
    .state(position),
    Spec::custom_with(
        "block.move_to_window",
        "Move Block to Window",
        "Move the block to another window, last there, where it plays on.",
        Scope::Block,
        &[Arg::required(
            "window",
            Kind::Text,
            "The window's id, its label, or its number, from 1.",
        )],
        send_to_window,
    )
    .tells(&[
        "block.moved",
        "window.layout_changed",
        "block.active_changed",
    ])
    .keywords(&["send", "other window"]),
    Spec::custom(
        "block.move_to_new_window",
        "Move Block to New Window",
        "Move the block to a window of its own beside this one, where it plays on.",
        Scope::Block,
        move_to_new_window,
    )
    .tells(&[
        "window.created",
        "block.moved",
        "window.layout_changed",
        "block.active_changed",
    ])
    .keywords(&["pop out", "detach", "separate", "own window"])
    .returns("{window}: the new window's id."),
    Spec::block_with(
        "block.copy_path",
        "Copy File Path",
        "Copy the path of the block's file, or the URL of its stream; hands back what was \
         copied.",
        &[],
        |_, player, _, cx| Ok(json!(player.copy_path(cx))),
    )
    .keywords(&["clipboard", "url", "location", "address"])
    .returns("The path or URL copied.")
    .keys(&["secondary-alt-c"]),
    Spec::block_with(
        "block.copy_time",
        "Copy Current Time",
        "Copy where the block's file is, as its bar shows it, such as 1:23:45; hands back \
         what was copied.",
        &[],
        |_, player, _, cx| Ok(json!(player.copy_time(cx))),
    )
    .keywords(&["clipboard", "position", "timestamp"])
    .returns("The time copied, such as 1:23:45."),
    Spec::block(
        "block.toggle_picture_in_picture",
        "Picture in Picture",
        "Play the block in a small window floating above every app's, or in its block again.",
        |player, window, cx| player.toggle_picture_in_picture(window, cx),
    )
    .tells(&["block.picture_in_picture_changed"])
    .keywords(&["pip", "float", "mini player"])
    .setting("picture_in_picture")
    .keys(on_mac(&["ctrl-cmd-p"], &[]))
    // macOS's.
    .only_if(|| cfg!(target_os = "macos")),
    Spec::block(
        "block.toggle_ui",
        "Show/Hide All",
        "Hide every panel over the block's video while any shows, else show them all.",
        |player, _, cx| change_over_video(player, OverVideo::all_toggled, cx),
    )
    .tells(&["block.ui_changed"])
    .keywords(&["hide", "interface", "ui", "panels", "clean"]),
    Spec::block(
        "block.toggle_controls",
        "Show Controls",
        "Show the seek bar and buttons at the bottom of the block, or hide them.",
        |player, _, cx| toggle_panel(player, |panel| &mut panel.controls, cx),
    )
    .tells(&["block.ui_changed"])
    .keywords(&["hide", "player controls", "seek bar", "bottom bar"])
    .state(|target, cx| shown_in_block(target, cx, |shown| shown.controls)),
    Spec::block(
        "block.toggle_top_bar",
        "Show Top Bar",
        "Show the file's name and buttons at the top of the block, or hide them.",
        |player, _, cx| toggle_panel(player, |panel| &mut panel.top_bar, cx),
    )
    .tells(&["block.ui_changed"])
    .keywords(&["hide", "header", "title", "buttons"])
    .state(|target, cx| shown_in_block(target, cx, |shown| shown.top_bar)),
    Spec::block(
        "block.toggle_track_switcher",
        "Show Track Switcher",
        "Show the reel of tracks a shortcut rolls to the audio, video or subtitles \
         picked, over the block, or switch without it.",
        |player, _, cx| toggle_panel(player, |panel| &mut panel.track_switcher, cx),
    )
    .tells(&["block.ui_changed"])
    .keywords(&["hide", "reel", "streams", "audio track", "subtitle track"])
    .state(|target, cx| shown_in_block(target, cx, |shown| shown.track_switcher)),
    Spec::block(
        "block.toggle_notes",
        "Show Notes",
        "Show notes on what a shortcut changed, like the volume, over the block, or \
         not. Notes on what's wrong still show.",
        |player, _, cx| toggle_panel(player, |panel| &mut panel.notes, cx),
    )
    .tells(&["block.ui_changed"])
    .keywords(&["hide", "osd", "on-screen display", "volume", "messages"])
    .state(|target, cx| shown_in_block(target, cx, |shown| shown.notes)),
    Spec::block(
        "block.toggle_console",
        "Show/Hide Console",
        "Show what the block's file is and how it plays, over its picture, or hide it.",
        |player, _, cx| player.toggle_console(cx),
    )
    .keywords(&["info", "stats", "codec", "debug"])
    .setting("console")
    .keys(&["~", "`"]),
];

/// The blocks' events.
pub const EVENTS: &[EventKind] = &[
    EventKind::block(
        "block.created",
        "A block came into a window, for a file to play in.",
        &[EventField::new("label", "Its label; none without one.")],
    )
    .causes(&[super::DROPPED]),
    EventKind::block(
        "block.moved",
        "A block moved to another window: those listening to the window it left hear it too.",
        &[
            EventField::new("from_window_id", "The id of the window it left."),
            EventField::new("from_window_title", "That window's title."),
        ],
    ),
    EventKind::block("block.closed", "A block closed, and the file in it.", &[])
        .causes(&["Plays quitting"]),
    EventKind::block(
        "block.label_changed",
        "A block was labeled, or its label taken away.",
        &[EventField::new(
            "label",
            "Its label; none once it's taken away.",
        )],
    ),
    EventKind::window(
        "block.active_changed",
        "Keys go to another block in a window, or to none.",
        &[
            EventField::new("block_id", "The block's id; none if keys go to none."),
            EventField::new(
                "block_title",
                "Its title, as its header shows it; none without a block.",
            ),
        ],
    )
    .causes(&["a click on a block or the mouse going to one"]),
    EventKind::block(
        "block.picture_in_picture_changed",
        "A block began playing in a small window floating above every app's, or in its block \
         again.",
        &[EventField::new(
            "picture_in_picture",
            "Whether it plays in the small window now.",
        )],
    )
    .causes(&["the small window's own buttons"]),
    EventKind::block(
        "block.ui_changed",
        "The panels shown over a block's video changed.",
        &[EventField::new("visible", super::VISIBLE)],
    )
    .causes(&[super::PANELS_SET]),
];

/// Close the block named, or the one keys go to.
fn close(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, block) = target::block(target, cx)?;
    let id = Entity::entity_id(&block);
    window
        .update(cx, |workspace, _, cx| workspace.close(id, cx))
        .map_err(|_| closed())?;
    done()
}

/// Close every block in the window of the block named, or the one keys go
/// to, but that one.
fn close_others(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, block) = target::block(target, cx)?;
    let id = Entity::entity_id(&block);
    window
        .update(cx, |workspace, _, cx| workspace.close_others(id, cx))
        .map_err(|_| closed())?;
    done()
}

#[derive(Deserialize)]
struct Rename {
    #[serde(default)]
    label: Option<String>,
}

/// Label the block named, or the one keys go to, with the label `args`
/// give, trimmed; with none, ask for one.
fn rename(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let Rename { label } = args.read()?;
    let (window, block) = target::block(target, cx)?;
    let id = Entity::entity_id(&block);
    let Some(label) = label else {
        window
            .update(cx, |workspace, window, cx| {
                window.activate_window();
                workspace.ask_for_block_label(id, window, cx);
            })
            .map_err(|_| closed())?;
        return done();
    };
    let label = label.trim();
    if block_label_taken(label, id, cx) {
        let message = format!("another block is called {label}");
        return Err(Error::bad_args(args.command, message));
    }
    let label = (!label.is_empty()).then(|| label.to_owned());
    block.update(cx, |player, cx| player.set_label(label, cx));
    done()
}

/// Where the block named, or the one keys go to, is in its window's grid,
/// from 1.
fn position(target: &Target, cx: &mut App) -> Outcome {
    let (window, block) = target::block(target, cx)?;
    let at = target::read(window, cx)?
        .players()
        .iter()
        .position(|other| *other == block)
        .ok_or_else(closed)?;
    Ok(json!(at + 1))
}

/// Fill the window with the block named, or the one keys go to, or show
/// every block again.
fn zoom(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, block) = target::block(target, cx)?;
    let id = Entity::entity_id(&block);
    window
        .update(cx, |workspace, window, cx| workspace.zoom(id, window, cx))
        .map_err(|_| closed())?;
    done()
}

/// Put the block in front, from the keyboard, its window too.
fn focus(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, block) = target::block(target, cx)?;
    let id = Entity::entity_id(&block);
    window
        .update(cx, |workspace, window, cx| {
            workspace.bring_block(id, cx);
            window.activate_window();
        })
        .map_err(|_| closed())?;
    done()
}

#[derive(Deserialize)]
struct Place {
    position: usize,
}

/// Put the block at the place `args` give, from 1.
fn put(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let Place { position } = args.read()?;
    let (window, block) = target::block(target, cx)?;
    let count = target::read(window, cx)?.block_count();
    if position > count {
        let message = format!("position is at most {count}, as many as the window has blocks");
        return Err(Error::bad_args(args.command, message));
    }
    let id = Entity::entity_id(&block);
    window
        .update(cx, |workspace, _, cx| {
            workspace.move_block_to(id, position - 1, cx)
        })
        .map_err(|_| closed())?;
    done()
}

#[derive(Deserialize)]
struct ToWindow {
    window: String,
}

/// Move the block to the window `args` name; it's there already if it's
/// its own.
fn send_to_window(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let ToWindow { window: name } = args.read()?;
    let (from, block) = target::block(target, cx)?;
    let named = Target {
        window: Some(name),
        block: None,
    };
    let to = target::window(&named, cx)?;
    if to == from {
        return done();
    }
    let id = Entity::entity_id(&block);
    let released = from
        .update(cx, |workspace, _, cx| workspace.release_block(id, cx))
        .map_err(|_| closed())?;
    if let Some(view) = released {
        to.update(cx, |workspace, _, cx| workspace.take_block(view, cx))
            .map_err(|_| closed())?;
    }
    done()
}

/// Move the block to a window of its own, handing back the window's id.
fn move_to_new_window(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, block) = target::block(target, cx)?;
    let id = Entity::entity_id(&block);
    let opened = window
        .update(cx, |workspace, window, cx| {
            workspace.move_to_new_window(id, window, cx)
        })
        .map_err(|_| closed())?
        .map_err(|err| Error::failed(format!("can't open a window: {err:#}")))?;
    match opened {
        Some(opened) => Ok(json!({"window": target::read(opened, cx)?.id().to_string()})),
        None => done(),
    }
}

/// Put the block in front at place `position` in its window's grid, from 1.
pub fn move_to(position: usize) -> Run {
    Run::with("block.move", json!({"position": position}))
}

/// Move the block in front to the window `name` names.
pub fn move_to_window(name: &str) -> Run {
    Run::with("block.move_to_window", json!({"window": name}))
}

/// Why a window couldn't be updated.
fn closed() -> Error {
    Error::failed("the window has closed")
}

/// Turn the panel `panel` picks on, or off, in `player`.
fn toggle_panel(player: &mut PlayerView, panel: Panel, cx: &mut Context<PlayerView>) {
    change_over_video(player, |shown| shown.toggled(panel), cx);
}

/// Change the panels over the video `player` shows with `change`.
fn change_over_video(
    player: &mut PlayerView,
    change: impl FnOnce(OverVideo) -> OverVideo,
    cx: &mut Context<PlayerView>,
) {
    let shown = change(player.over_video());
    player.set_over_video(shown, cx);
}

/// Whether the block `target` names, or the one keys go to, shows the panel
/// `panel` reads.
fn shown_in_block(target: &Target, cx: &mut App, panel: fn(OverVideo) -> bool) -> Outcome {
    target::read_block(target, cx, |player, _| json!(panel(player.over_video())))
}
