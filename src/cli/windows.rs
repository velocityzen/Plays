//! `plays ls`, `plays window …` and `plays tile …`: the windows and their
//! tiles, listed, inspected and changed by the commands the app has for
//! them. Those named by number are looked up by id first, so closing or
//! moving one doesn't change which the next name names.

use serde_json::{Value, json};

use super::args::{Place, Tile, Window};
use super::{Done, Session, Stop, listed, show};
use crate::api::Target;

/// The windows, each with its tiles.
pub fn ls(session: &mut Session) -> Done {
    let mut windows = session.call_on("window.ls", Value::Null, Target::default())?;
    for window in windows.as_array_mut().into_iter().flatten() {
        let tiles = session.call_on("tile.ls", Value::Null, in_window(&window["id"]))?;
        window["tiles"] = tiles;
    }
    if session.options.json {
        return session.result(&windows);
    }
    for window in listed(&windows) {
        let label = show::cell(&window["label"]);
        let line = format!(
            "{:1} {}  {}  {label}",
            show::front(window),
            window["number"],
            show::text(&window["id"])
        );
        writeln!(session.out, "{}", line.trim_end())?;
        for tile in listed(&window["tiles"]) {
            let line = format!(
                "    {:1} {}  {}  {}  {}",
                show::front(tile),
                tile["number"],
                show::text(&tile["id"]),
                name(tile),
                playing(tile)
            );
            writeln!(session.out, "{}", line.trim_end())?;
        }
    }
    Ok(())
}

pub fn window(session: &mut Session, window: Window) -> Done {
    match window {
        Window::Ls => {
            let windows = session.call_on("window.ls", Value::Null, Target::default())?;
            if session.options.json {
                return session.result(&windows);
            }
            let rows: Vec<Vec<String>> = listed(&windows)
                .map(|window| {
                    vec![
                        show::front(window),
                        window["number"].to_string(),
                        show::text(&window["id"]),
                        show::cell(&window["label"]),
                        window["tiles"].to_string(),
                    ]
                })
                .collect();
            Ok(show::table(
                session.out,
                &["", "#", "ID", "LABEL", "TILES"],
                &rows,
            )?)
        }
        Window::Inspect { window } => {
            let target = named_window(session, window);
            let inspected = session.call_on("window.inspect", Value::Null, target)?;
            Ok(show::pretty(session.out, &inspected)?)
        }
        Window::Focus { window } => {
            let command = match window.as_deref() {
                Some("next") => "window.next",
                Some("prev" | "previous") => "window.previous",
                _ => "window.focus",
            };
            let target = match command {
                "window.focus" => named_window(session, window),
                _ => Target::default(),
            };
            session.call_on(command, Value::Null, target)?;
            Ok(())
        }
        Window::New { label } => {
            let opened = session.call_on("window.new", Value::Null, Target::default())?;
            if let Some(label) = label {
                let named = in_window(&opened["window"]);
                session.call_on("window.rename", json!({"label": label}), named)?;
            }
            if session.options.json {
                return session.result(&opened);
            }
            session.result(&opened["window"])
        }
        Window::Close { windows } => {
            for target in window_ids(session, windows)? {
                session.call_on("window.close", Value::Null, target)?;
            }
            Ok(())
        }
        Window::Rename { mut names } => {
            let label = names.pop().unwrap_or_default();
            let target = named_window(session, names.pop());
            session.call_on("window.rename", json!({"label": label}), target)?;
            Ok(())
        }
        Window::Move { window, to } => {
            let mover = named_window(session, window);
            let moving = session.call_on("window.inspect", Value::Null, mover)?;
            let position = match place(&to) {
                Ok(index) => index,
                Err((other, after)) => {
                    let named = named_window(session, Some(other));
                    let other = session.call_on("window.inspect", Value::Null, named)?;
                    beside(&moving, &other, after, "window")?
                }
            };
            let id = in_window(&moving["id"]);
            session.call_on("window.move", json!({"position": position}), id)?;
            Ok(())
        }
    }
}

pub fn tile(session: &mut Session, tile: Tile) -> Done {
    match tile {
        Tile::Ls => {
            let tiles = session.call("tile.ls", Value::Null)?;
            if session.options.json {
                return session.result(&tiles);
            }
            let rows: Vec<Vec<String>> = listed(&tiles)
                .map(|tile| {
                    vec![
                        show::front(tile),
                        tile["number"].to_string(),
                        show::text(&tile["id"]),
                        name(tile),
                        playing(tile).to_owned(),
                    ]
                })
                .collect();
            Ok(show::table(
                session.out,
                &["", "#", "ID", "NAME", "STATE"],
                &rows,
            )?)
        }
        Tile::Inspect { tile } => {
            let target = named_tile(session, tile);
            let inspected = session.call_on("tile.inspect", Value::Null, target)?;
            Ok(show::pretty(session.out, &inspected)?)
        }
        Tile::Focus { tile } => run_on_tile(session, "tile.focus", tile),
        Tile::Zoom { tile } => run_on_tile(session, "tile.zoom", tile),
        Tile::Close { tiles } => {
            for target in tile_ids(session, tiles)? {
                session.call_on("tile.close", Value::Null, target)?;
            }
            Ok(())
        }
        Tile::Rename { mut names } => {
            let label = names.pop().unwrap_or_default();
            let target = named_tile(session, names.pop());
            session.call_on("tile.rename", json!({"label": label}), target)?;
            Ok(())
        }
        Tile::Move { tile, to } => {
            let mover = named_tile(session, tile);
            let moving = session.call_on("tile.inspect", Value::Null, mover)?;
            let position = match place(&to) {
                Ok(index) => index,
                Err((other, after)) => {
                    let named = named_tile(session, Some(other));
                    let other = session.call_on("tile.inspect", Value::Null, named)?;
                    if other["window"] != moving["window"] {
                        return Err(Stop::Usage(format!(
                            "{} is in another window: move tiles between windows with \
                             plays do tile.move_to_window",
                            show::text(&other["id"])
                        )));
                    }
                    beside(&moving, &other, after, "tile")?
                }
            };
            let id = Target {
                window: None,
                tile: Some(show::text(&moving["id"])),
            };
            session.call_on("tile.move", json!({"position": position}), id)?;
            Ok(())
        }
    }
}

/// Run `command` on the tile `tile` names, or `-t`'s.
fn run_on_tile(session: &mut Session, command: &str, tile: Option<String>) -> Done {
    let target = named_tile(session, tile);
    session.call_on(command, Value::Null, target)?;
    Ok(())
}

/// The window `id` is.
fn in_window(id: &Value) -> Target {
    Target {
        window: Some(show::text(id)),
        tile: None,
    }
}

/// The window `name` names, or `-w`'s.
fn named_window(session: &Session, name: Option<String>) -> Target {
    Target {
        window: name.or_else(|| session.options.window.clone()),
        tile: None,
    }
}

/// The tile `name` names, in `-w`'s window if named, or `-t`'s.
fn named_tile(session: &Session, name: Option<String>) -> Target {
    Target {
        window: session.options.window.clone(),
        tile: name.or_else(|| session.options.tile.clone()),
    }
}

/// The windows `names` name, by id, found before any changes: or `-w`'s,
/// with none.
fn window_ids(session: &mut Session, names: Vec<String>) -> Result<Vec<Target>, Stop> {
    if names.is_empty() {
        return Ok(vec![named_window(session, None)]);
    }
    names
        .into_iter()
        .map(|name| {
            let inspected = session.call_on(
                "window.inspect",
                Value::Null,
                named_window(session, Some(name)),
            )?;
            Ok(in_window(&inspected["id"]))
        })
        .collect()
}

/// The tiles `names` name, by id, found before any changes: or `-t`'s,
/// with none.
fn tile_ids(session: &mut Session, names: Vec<String>) -> Result<Vec<Target>, Stop> {
    if names.is_empty() {
        return Ok(vec![named_tile(session, None)]);
    }
    names
        .into_iter()
        .map(|name| {
            let inspected =
                session.call_on("tile.inspect", Value::Null, named_tile(session, Some(name)))?;
            Ok(Target {
                window: None,
                tile: Some(show::text(&inspected["id"])),
            })
        })
        .collect()
}

/// Where `to` puts it: at a place from 1, or beside the one named, after
/// it or before.
fn place(to: &Place) -> Result<usize, (String, bool)> {
    match (to.index, &to.before, &to.after) {
        (Some(index), ..) => Ok(index),
        (None, Some(before), _) => Err((before.clone(), false)),
        (None, None, Some(after)) => Err((after.clone(), true)),
        // clap asks for one.
        (None, None, None) => Ok(0),
    }
}

/// The place, from 1, that puts `moving` just after `other`, or just
/// before it, the others between moving over one.
fn beside(moving: &Value, other: &Value, after: bool, what: &str) -> Result<usize, Stop> {
    if moving["id"] == other["id"] {
        return Err(Stop::Usage(format!("a {what} can't move beside itself")));
    }
    let number = |value: &Value| value["number"].as_u64().unwrap_or(0) as usize;
    let (from, to) = (number(moving), number(other));
    // Leaving its place moves those after it up one.
    Ok(match (from < to, after) {
        (true, true) => to,
        (true, false) => to - 1,
        (false, true) => to + 1,
        (false, false) => to,
    })
}

/// What a tile is called: its label, or its file's name.
fn name(tile: &Value) -> String {
    match &tile["label"] {
        Value::Null => show::cell(&tile["title"]),
        label => show::text(label),
    }
}

/// Whether a tile plays.
fn playing(tile: &Value) -> &'static str {
    if tile["playing"] == true {
        "playing"
    } else {
        "paused"
    }
}

#[cfg(test)]
#[path = "tests/windows.rs"]
mod tests;
