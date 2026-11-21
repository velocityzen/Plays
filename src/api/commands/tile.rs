//! `tile`: the tiles in a window: closing them, going from one to another,
//! moving them about the grid, and what shows over or apart from one.

use gpui::{App, Entity};
use serde::Deserialize;
use serde_json::json;

use super::done;
use crate::api::Error;
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Args, Outcome, Reach, Scope, Spec, on_mac};
use crate::api::target::{self, Target};
use crate::ui::workspace::Toward;

pub const COMMANDS: &[Spec] = &[
    Spec::custom(
        "tile.close",
        "Close File",
        "Close the tile's file. The window stays, empty after the last.",
        Scope::Tile,
        close,
    )
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
    .keywords(&["clear", "remove", "stop"])
    .keys(&["secondary-shift-w"])
    .reach(Reach::Files),
    Spec::window(
        "tile.focus.left",
        "Focus Tile to the Left",
        "Send keys to the tile left of the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Left, cx),
    )
    .setting("focus_left")
    .keys(&["secondary-alt-left"]),
    Spec::window(
        "tile.focus.right",
        "Focus Tile to the Right",
        "Send keys to the tile right of the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Right, cx),
    )
    .setting("focus_right")
    .keys(&["secondary-alt-right"]),
    Spec::window(
        "tile.focus.up",
        "Focus Tile Above",
        "Send keys to the tile above the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Up, cx),
    )
    .setting("focus_up")
    .keys(&["secondary-alt-up"]),
    Spec::window(
        "tile.focus.down",
        "Focus Tile Below",
        "Send keys to the tile below the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Down, cx),
    )
    .setting("focus_down")
    .keys(&["secondary-alt-down"]),
    Spec::window(
        "tile.focus.previous",
        "Focus Previous Tile",
        "Send keys to the tile before the one they go to, the last before the first.",
        |workspace, _, cx| workspace.focus_tile(Toward::Previous, cx),
    )
    .setting("focus_previous_tile")
    .keys(&["secondary-["]),
    Spec::window(
        "tile.focus.next",
        "Focus Next Tile",
        "Send keys to the tile after the one they go to, the first after the last.",
        |workspace, _, cx| workspace.focus_tile(Toward::Next, cx),
    )
    .setting("focus_next_tile")
    .keys(&["secondary-]"]),
    Spec::window(
        "tile.move.left",
        "Move Tile Left",
        "Swap the tile keys go to with the one to its left.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Left, cx),
    )
    .setting("move_tile_left")
    .keys(&["secondary-alt-shift-left"]),
    Spec::window(
        "tile.move.right",
        "Move Tile Right",
        "Swap the tile keys go to with the one to its right.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Right, cx),
    )
    .setting("move_tile_right")
    .keys(&["secondary-alt-shift-right"]),
    Spec::window(
        "tile.move.up",
        "Move Tile Up",
        "Swap the tile keys go to with the one above it.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Up, cx),
    )
    .setting("move_tile_up")
    .keys(&["secondary-alt-shift-up"]),
    Spec::window(
        "tile.move.down",
        "Move Tile Down",
        "Swap the tile keys go to with the one below it.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Down, cx),
    )
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
    .keywords(&["expand", "enlarge", "full", "restore"]),
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
        move_to,
    )
    .keywords(&["reorder", "place", "drag", "swap"]),
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
        move_to_window,
    )
    .keywords(&["send", "other window"]),
    Spec::custom(
        "tile.move_to_new_window",
        "Move Tile to New Window",
        "Move the tile to a window of its own beside this one, where it plays on.",
        Scope::Tile,
        move_to_new_window,
    )
    .keywords(&["pop out", "detach", "separate", "own window"]),
    Spec::tile(
        "tile.toggle_picture_in_picture",
        "Picture in Picture",
        "Play the tile in a small window floating above every app's, or in its tile again.",
        |player, window, cx| player.toggle_picture_in_picture(window, cx),
    )
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

/// Close the tile named, or the one keys go to.
fn close(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let id = Entity::entity_id(&tile);
    window
        .update(cx, |workspace, _, cx| workspace.close(id, cx))
        .map_err(|_| closed())?;
    done()
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

#[derive(Deserialize)]
struct Place {
    position: usize,
}

/// Put the tile at the place `args` give, from 1.
fn move_to(args: Args, target: &Target, cx: &mut App) -> Outcome {
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
fn move_to_window(args: Args, target: &Target, cx: &mut App) -> Outcome {
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

/// Why a window couldn't be updated.
fn closed() -> Error {
    Error::failed("the window has closed")
}
