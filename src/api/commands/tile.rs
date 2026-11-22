//! `tile`: the tiles in a window: closing them, going from one to another,
//! moving them about the grid, and what shows over or apart from one.

use gpui::{App, Entity};
use serde::Deserialize;
use serde_json::{Value, json};

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Args, Outcome, Reach, Scope, Spec, on_mac};
use crate::api::target::{self, Target};
use crate::api::{Error, Run};
use crate::ui::workspace::{Toward, tile_label_taken};

pub const COMMANDS: &[Spec] = &[
    Spec::query(
        "tile.ls",
        "List Tiles",
        "List the tiles in the window, in the grid's order.",
        Scope::Window,
        "[{id, label, title, file, url, playing, number, front}]: each tile's id and label \
         (or null), its file's name, the file's path or the stream's URL (without a user, \
         password or query; the other null), whether it plays, its place from 1, and \
         whether keys go to it.",
        |_, target, cx| {
            let window = target::window(target, cx)?;
            Ok(Value::from(target::read(window, cx)?.tiles_listed(cx)))
        },
    )
    .keywords(&["list", "ls", "tiles", "files"]),
    Spec::query(
        "tile.inspect",
        "Inspect Tile",
        "Say all about the tile: what it plays, where it is, and how it's set to play.",
        Scope::Tile,
        "{as tile.ls, window, window_title, position, duration, frame_rate, ended, \
         volume, muted, looping, tracks: {video, audio, subtitles: [{number, label, \
         language, title, codec, format, default, forced, hearing_impaired, file}]}, \
         picked: {video, audio, subtitles}, delays: {audio, subtitles}, picture: {turns, \
         mirrored, aspect_ratio, crop}, chapter, chapters: [{number, name, start, end}], \
         casting: {id, kind, name} or null}: its window's id and title, times in seconds \
         (duration null for a live stream), frames a second (null for sound alone), \
         volume from 0 to 100, tracks and chapters numbered from 1, the tracks picked and \
         the chapter by number or null, delays in seconds, and quarter turns clockwise.",
        |_, target, cx| {
            let (window, tile) = target::tile(target, cx)?;
            target::read(window, cx)?
                .inspect_tile(&tile, cx)
                .ok_or_else(closed)
        },
    )
    .keywords(&["info", "details", "describe", "tracks", "position"]),
    Spec::custom(
        "tile.focus",
        "Go to Tile",
        "Put the tile in front, keys going to it, and bring its window to the front.",
        Scope::Tile,
        focus,
    )
    .tells(&["active_window_changed", "active_tile_changed"])
    .keywords(&["switch", "select", "show", "activate"])
    // The tile in front is in front already.
    .needs_target(),
    Spec::custom(
        "tile.close",
        "Close File",
        "Close the tile's file. The window stays, empty after the last.",
        Scope::Tile,
        close,
    )
    .tells(&[
        "tile_closed",
        "window_layout_changed",
        "active_tile_changed",
    ])
    .keywords(&["remove", "stop"])
    .keys(&["secondary-w"])
    // Typing a URL over the tiles too, but where a key that works anywhere
    // is this one's as well, as Close Window's ⌘W is, that one wins.
    .reach(Reach::Files),
    Spec::tiles(
        "tile.close_all",
        "Close All Files",
        "Close every file in the window. The window stays, empty.",
        |workspace, _, cx| workspace.close_all(cx),
    )
    .tells(&[
        "tile_closed",
        "window_layout_changed",
        "active_tile_changed",
    ])
    .keywords(&["clear", "remove", "stop"])
    .keys(&["secondary-shift-w"])
    .reach(Reach::Files),
    Spec::custom(
        "tile.close_others",
        "Close Other Files",
        "Close every file in the window but the tile's.",
        Scope::Tile,
        close_others,
    )
    .tells(&["tile_closed", "window_layout_changed"])
    .keywords(&["close all", "others", "keep", "remove"]),
    Spec::window(
        "tile.focus.left",
        "Focus Tile to the Left",
        "Send keys to the tile left of the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Left, cx),
    )
    .tells(&["active_tile_changed"])
    .setting("focus_left")
    .keys(&["secondary-alt-left"]),
    Spec::window(
        "tile.focus.right",
        "Focus Tile to the Right",
        "Send keys to the tile right of the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Right, cx),
    )
    .tells(&["active_tile_changed"])
    .setting("focus_right")
    .keys(&["secondary-alt-right"]),
    Spec::window(
        "tile.focus.up",
        "Focus Tile Above",
        "Send keys to the tile above the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Up, cx),
    )
    .tells(&["active_tile_changed"])
    .setting("focus_up")
    .keys(&["secondary-alt-up"]),
    Spec::window(
        "tile.focus.down",
        "Focus Tile Below",
        "Send keys to the tile below the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Down, cx),
    )
    .tells(&["active_tile_changed"])
    .setting("focus_down")
    .keys(&["secondary-alt-down"]),
    Spec::window(
        "tile.focus.previous",
        "Focus Previous Tile",
        "Send keys to the tile before the one they go to, the last before the first.",
        |workspace, _, cx| workspace.focus_tile(Toward::Previous, cx),
    )
    .tells(&["active_tile_changed"])
    .setting("focus_previous_tile")
    .keys(&["secondary-["]),
    Spec::window(
        "tile.focus.next",
        "Focus Next Tile",
        "Send keys to the tile after the one they go to, the first after the last.",
        |workspace, _, cx| workspace.focus_tile(Toward::Next, cx),
    )
    .tells(&["active_tile_changed"])
    .setting("focus_next_tile")
    .keys(&["secondary-]"]),
    Spec::window(
        "tile.back",
        "Go Back One Tile",
        "Send keys to the tile clicked or gone to before the one they go to; again, the \
         one before that, and so on, until something else is done.",
        |workspace, _, cx| workspace.back_tile(cx),
    )
    .tells(&["active_tile_changed"])
    .keywords(&["history", "previous", "last", "return", "focus"]),
    Spec::window(
        "tile.recent",
        "Go to Recent Tile",
        "Send keys to the tile clicked or gone to last but the one they go to; again, \
         back to that one.",
        |workspace, _, cx| workspace.recent_tile(cx),
    )
    .tells(&["active_tile_changed"])
    .keywords(&["history", "last", "toggle", "alternate", "switch", "focus"]),
    Spec::window(
        "tile.move.left",
        "Move Tile Left",
        "Swap the tile keys go to with the one to its left.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Left, cx),
    )
    .tells(&["window_layout_changed"])
    .setting("move_tile_left")
    .keys(&["secondary-alt-shift-left"]),
    Spec::window(
        "tile.move.right",
        "Move Tile Right",
        "Swap the tile keys go to with the one to its right.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Right, cx),
    )
    .tells(&["window_layout_changed"])
    .setting("move_tile_right")
    .keys(&["secondary-alt-shift-right"]),
    Spec::window(
        "tile.move.up",
        "Move Tile Up",
        "Swap the tile keys go to with the one above it.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Up, cx),
    )
    .tells(&["window_layout_changed"])
    .setting("move_tile_up")
    .keys(&["secondary-alt-shift-up"]),
    Spec::window(
        "tile.move.down",
        "Move Tile Down",
        "Swap the tile keys go to with the one below it.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Down, cx),
    )
    .tells(&["window_layout_changed"])
    .setting("move_tile_down")
    .keys(&["secondary-alt-shift-down"]),
    Spec::custom(
        "tile.zoom",
        "Maximize Tile",
        "Fill the window with the tile, or show every tile again; a window's only \
         tile fills the screen instead.",
        Scope::Tile,
        zoom,
    )
    .tells(&["full_screen_changed", "window_layout_changed"])
    .keywords(&["expand", "enlarge", "full", "restore"])
    .keys(&["secondary-shift-enter"]),
    Spec::custom_with(
        "tile.rename",
        "Rename Tile…",
        "Label the tile: its header shows the label in place of the file's name, and it \
         can be named by it. A blank one takes the label away; with none given, the \
         window asks for one.",
        Scope::Tile,
        &[Arg::optional(
            "label",
            Kind::Text,
            "What to call it; no two tiles are called the same, in whichever windows.",
        )],
        rename,
    )
    .tells(&["tile_label_changed"])
    .keywords(&["name", "label", "title"]),
    Spec::custom_with(
        "tile.move",
        "Move Tile To",
        "Put the tile in another place in its window's grid, the tiles between moving \
         over, as dragging it onto another does.",
        Scope::Tile,
        &[Arg::required(
            "position",
            Kind::Integer { min: 1 },
            "Where, from 1, the first tile's place.",
        )],
        put,
    )
    .tells(&["window_layout_changed"])
    .keywords(&["reorder", "place", "drag", "swap"])
    .state(position),
    Spec::custom_with(
        "tile.move_to_window",
        "Move Tile to Window",
        "Move the tile to another window, last there, where it plays on.",
        Scope::Tile,
        &[Arg::required(
            "window",
            Kind::Text,
            "The window's id, its label, or its number, from 1.",
        )],
        send_to_window,
    )
    .tells(&["tile_moved", "window_layout_changed", "active_tile_changed"])
    .keywords(&["send", "other window"]),
    Spec::custom(
        "tile.move_to_new_window",
        "Move Tile to New Window",
        "Move the tile to a window of its own beside this one, where it plays on.",
        Scope::Tile,
        move_to_new_window,
    )
    .tells(&[
        "window_created",
        "tile_moved",
        "window_layout_changed",
        "active_tile_changed",
    ])
    .keywords(&["pop out", "detach", "separate", "own window"])
    .returns("{window}: the new window's id."),
    Spec::tile_with(
        "tile.copy_path",
        "Copy File Path",
        "Copy the path of the tile's file, or the URL of its stream; hands back what was \
         copied.",
        &[],
        |_, player, _, cx| Ok(json!(player.copy_path(cx))),
    )
    .keywords(&["clipboard", "url", "location", "address"])
    .returns("The path or URL copied.")
    .keys(&["secondary-alt-c"]),
    Spec::tile_with(
        "tile.copy_time",
        "Copy Current Time",
        "Copy where the tile's file is, as its bar shows it, such as 1:23:45; hands back \
         what was copied.",
        &[],
        |_, player, _, cx| Ok(json!(player.copy_time(cx))),
    )
    .keywords(&["clipboard", "position", "timestamp"])
    .returns("The time copied, such as 1:23:45."),
    Spec::tile(
        "tile.toggle_picture_in_picture",
        "Picture in Picture",
        "Play the tile in a small window floating above every app's, or in its tile again.",
        |player, window, cx| player.toggle_picture_in_picture(window, cx),
    )
    .tells(&["picture_in_picture_changed"])
    .keywords(&["pip", "float", "mini player"])
    .setting("picture_in_picture")
    .keys(on_mac(&["ctrl-cmd-p"], &[]))
    // macOS's.
    .only_if(|| cfg!(target_os = "macos")),
    Spec::tile(
        "tile.toggle_console",
        "Show/Hide Console",
        "Show what the tile's file is and how it plays, over its picture, or hide it.",
        |player, _, cx| player.toggle_console(cx),
    )
    .keywords(&["info", "stats", "codec", "debug"])
    .setting("console")
    .keys(&["~", "`"]),
];

/// The tiles' events.
pub const EVENTS: &[EventKind] = &[
    EventKind::tile(
        "tile_created",
        "A tile came into a window, for a file to play in.",
        &[EventField::new("label", "Its label; none without one.")],
    )
    .causes(&[super::DROPPED]),
    EventKind::tile(
        "tile_moved",
        "A tile moved to another window: those listening to the window it left hear it too.",
        &[
            EventField::new("from_window_id", "The id of the window it left."),
            EventField::new("from_window_title", "That window's title."),
        ],
    ),
    EventKind::tile("tile_closed", "A tile closed, and the file in it.", &[])
        .causes(&["Plays quitting"]),
    EventKind::tile(
        "tile_label_changed",
        "A tile was labeled, or its label taken away.",
        &[EventField::new(
            "label",
            "Its label; none once it's taken away.",
        )],
    ),
    EventKind::window(
        "active_tile_changed",
        "Keys go to another tile in a window, or to none.",
        &[
            EventField::new("tile_id", "The tile's id; none if keys go to none."),
            EventField::new(
                "tile_title",
                "Its title, as its header shows it; none without a tile.",
            ),
        ],
    )
    .causes(&["a click on a tile or the mouse going to one"]),
    EventKind::tile(
        "picture_in_picture_changed",
        "A tile began playing in a small window floating above every app's, or in its tile \
         again.",
        &[EventField::new(
            "picture_in_picture",
            "Whether it plays in the small window now.",
        )],
    )
    .causes(&["the small window's own buttons"]),
];

/// Close the tile named, or the one keys go to.
fn close(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let id = Entity::entity_id(&tile);
    window
        .update(cx, |workspace, _, cx| workspace.close(id, cx))
        .map_err(|_| closed())?;
    done()
}

/// Close every tile in the window of the tile named, or the one keys go
/// to, but that one.
fn close_others(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let id = Entity::entity_id(&tile);
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

/// Label the tile named, or the one keys go to, with the label `args`
/// give, trimmed; with none, ask for one.
fn rename(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let Rename { label } = args.read()?;
    let (window, tile) = target::tile(target, cx)?;
    let id = Entity::entity_id(&tile);
    let Some(label) = label else {
        window
            .update(cx, |workspace, window, cx| {
                window.activate_window();
                workspace.ask_for_tile_label(id, window, cx);
            })
            .map_err(|_| closed())?;
        return done();
    };
    let label = label.trim();
    if tile_label_taken(label, id, cx) {
        let message = format!("another tile is called {label}");
        return Err(Error::bad_args(args.command, message));
    }
    let label = (!label.is_empty()).then(|| label.to_owned());
    tile.update(cx, |player, cx| player.set_label(label, cx));
    done()
}

/// Where the tile named, or the one keys go to, is in its window's grid,
/// from 1.
fn position(target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let at = target::read(window, cx)?
        .players()
        .iter()
        .position(|other| *other == tile)
        .ok_or_else(closed)?;
    Ok(json!(at + 1))
}

/// Fill the window with the tile named, or the one keys go to, or show
/// every tile again.
fn zoom(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let id = Entity::entity_id(&tile);
    window
        .update(cx, |workspace, window, cx| workspace.zoom(id, window, cx))
        .map_err(|_| closed())?;
    done()
}

/// Put the tile in front, from the keyboard, its window too.
fn focus(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let id = Entity::entity_id(&tile);
    window
        .update(cx, |workspace, window, cx| {
            workspace.bring_tile(id, cx);
            window.activate_window();
        })
        .map_err(|_| closed())?;
    done()
}

#[derive(Deserialize)]
struct Place {
    position: usize,
}

/// Put the tile at the place `args` give, from 1.
fn put(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let Place { position } = args.read()?;
    let (window, tile) = target::tile(target, cx)?;
    let count = target::read(window, cx)?.tile_count();
    if position > count {
        let message = format!("position is at most {count}, as many as the window has tiles");
        return Err(Error::bad_args(args.command, message));
    }
    let id = Entity::entity_id(&tile);
    window
        .update(cx, |workspace, _, cx| {
            workspace.move_tile_to(id, position - 1, cx)
        })
        .map_err(|_| closed())?;
    done()
}

#[derive(Deserialize)]
struct ToWindow {
    window: String,
}

/// Move the tile to the window `args` name; it's there already if it's
/// its own.
fn send_to_window(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let ToWindow { window: name } = args.read()?;
    let (from, tile) = target::tile(target, cx)?;
    let named = Target {
        window: Some(name),
        tile: None,
    };
    let to = target::window(&named, cx)?;
    if to == from {
        return done();
    }
    let id = Entity::entity_id(&tile);
    let released = from
        .update(cx, |workspace, _, cx| workspace.release_tile(id, cx))
        .map_err(|_| closed())?;
    if let Some(view) = released {
        to.update(cx, |workspace, _, cx| workspace.take_tile(view, cx))
            .map_err(|_| closed())?;
    }
    done()
}

/// Move the tile to a window of its own, handing back the window's id.
fn move_to_new_window(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let id = Entity::entity_id(&tile);
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

/// Put the tile in front at place `position` in its window's grid, from 1.
pub fn move_to(position: usize) -> Run {
    Run::with("tile.move", json!({"position": position}))
}

/// Move the tile in front to the window `name` names.
pub fn move_to_window(name: &str) -> Run {
    Run::with("tile.move_to_window", json!({"window": name}))
}

/// Why a window couldn't be updated.
fn closed() -> Error {
    Error::failed("the window has closed")
}
