//! `tile`: the tiles in a window: closing them, going from one to another,
//! moving them about the grid, and what shows over or apart from one.

use gpui::{App, Entity};

use super::done;
use crate::api::Error;
use crate::api::registry::{Args, Outcome, Scope, Spec};
use crate::api::target::{self, Target};
use crate::hotkeys::Command;
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
    .keys(&["secondary-w"]),
    Spec::tiles(
        "tile.close_all",
        "Close All Files",
        "Close every file in the window. The window stays, empty.",
        |workspace, _, cx| workspace.close_all(cx),
    )
    .keywords(&["clear", "remove", "stop"])
    .keys(&["secondary-shift-w"]),
    Spec::window(
        "tile.focus.left",
        "Focus Tile to the Left",
        "Send keys to the tile left of the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Left, cx),
    )
    .hotkey(Command::FocusLeft),
    Spec::window(
        "tile.focus.right",
        "Focus Tile to the Right",
        "Send keys to the tile right of the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Right, cx),
    )
    .hotkey(Command::FocusRight),
    Spec::window(
        "tile.focus.up",
        "Focus Tile Above",
        "Send keys to the tile above the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Up, cx),
    )
    .hotkey(Command::FocusUp),
    Spec::window(
        "tile.focus.down",
        "Focus Tile Below",
        "Send keys to the tile below the one they go to.",
        |workspace, _, cx| workspace.focus_tile(Toward::Down, cx),
    )
    .hotkey(Command::FocusDown),
    Spec::window(
        "tile.focus.previous",
        "Focus Previous Tile",
        "Send keys to the tile before the one they go to, the last before the first.",
        |workspace, _, cx| workspace.focus_tile(Toward::Previous, cx),
    )
    .hotkey(Command::FocusPreviousTile),
    Spec::window(
        "tile.focus.next",
        "Focus Next Tile",
        "Send keys to the tile after the one they go to, the first after the last.",
        |workspace, _, cx| workspace.focus_tile(Toward::Next, cx),
    )
    .hotkey(Command::FocusNextTile),
    Spec::window(
        "tile.move.left",
        "Move Tile Left",
        "Swap the tile keys go to with the one to its left.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Left, cx),
    )
    .hotkey(Command::MoveTileLeft),
    Spec::window(
        "tile.move.right",
        "Move Tile Right",
        "Swap the tile keys go to with the one to its right.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Right, cx),
    )
    .hotkey(Command::MoveTileRight),
    Spec::window(
        "tile.move.up",
        "Move Tile Up",
        "Swap the tile keys go to with the one above it.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Up, cx),
    )
    .hotkey(Command::MoveTileUp),
    Spec::window(
        "tile.move.down",
        "Move Tile Down",
        "Swap the tile keys go to with the one below it.",
        |workspace, _, cx| workspace.move_front_tile(Toward::Down, cx),
    )
    .hotkey(Command::MoveTileDown),
    Spec::tile(
        "tile.toggle_picture_in_picture",
        "Picture in Picture",
        "Play the tile in a small window floating above every app's, or in its tile again.",
        |player, window, cx| player.toggle_picture_in_picture(window, cx),
    )
    .keywords(&["pip", "float", "mini player"])
    .hotkey(Command::PictureInPicture)
    // macOS's.
    .only_if(|| cfg!(target_os = "macos")),
    Spec::tile(
        "tile.toggle_console",
        "Show/Hide Console",
        "Show what the tile's file is and how it plays, over its picture, or hide it.",
        |player, _, cx| player.toggle_console(cx),
    )
    .keywords(&["info", "stats", "codec", "debug"])
    .hotkey(Command::Console),
];

/// Close the tile named, or the one keys go to.
fn close(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let id = Entity::entity_id(&tile);
    window
        .update(cx, |workspace, _, cx| workspace.close(id, cx))
        .map_err(|_| Error::failed("the window has closed"))?;
    done()
}
