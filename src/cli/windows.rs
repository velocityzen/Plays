//! `plays ls`, `plays window …` and `plays block …`: the windows and their
//! blocks, listed, inspected and changed by the commands the app has for
//! them. Those named by number are looked up by id first, so closing or
//! moving one doesn't change which the next name names.

use serde_json::{Value, json};

use super::args::{Block, Place, Window};
use super::{Done, Session, Stop, listed, show};
use crate::api::Target;

/// The windows, each with its blocks.
pub fn ls(session: &mut Session) -> Done {
    let mut windows = session.call_on("window.ls", Value::Null, Target::default())?;
    for window in windows.as_array_mut().into_iter().flatten() {
        let blocks = session.call_on("block.ls", Value::Null, in_window(&window["id"]))?;
        window["blocks"] = blocks;
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
        for block in listed(&window["blocks"]) {
            let line = format!(
                "    {:1} {}  {}  {}  {}",
                show::front(block),
                block["number"],
                show::text(&block["id"]),
                name(block),
                state(block)
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
                        window["blocks"].to_string(),
                    ]
                })
                .collect();
            Ok(show::table(
                session.out,
                &["", "#", "ID", "LABEL", "BLOCKS"],
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

pub fn block(session: &mut Session, block: Block) -> Done {
    match block {
        Block::Ls => {
            let blocks = session.call("block.ls", Value::Null)?;
            if session.options.json {
                return session.result(&blocks);
            }
            let rows: Vec<Vec<String>> = listed(&blocks)
                .map(|block| {
                    vec![
                        show::front(block),
                        block["number"].to_string(),
                        show::text(&block["id"]),
                        name(block),
                        state(block).to_owned(),
                    ]
                })
                .collect();
            Ok(show::table(
                session.out,
                &["", "#", "ID", "NAME", "STATE"],
                &rows,
            )?)
        }
        Block::Inspect { block } => {
            let target = named_block(session, block);
            let inspected = session.call_on("block.inspect", Value::Null, target)?;
            Ok(show::pretty(session.out, &inspected)?)
        }
        Block::Focus { block } => run_on_block(session, "block.focus", block),
        Block::Zoom { block } => run_on_block(session, "block.zoom", block),
        Block::Close { blocks } => {
            for target in block_ids(session, blocks)? {
                session.call_on("block.close", Value::Null, target)?;
            }
            Ok(())
        }
        Block::Rename { mut names } => {
            let label = names.pop().unwrap_or_default();
            let target = named_block(session, names.pop());
            session.call_on("block.rename", json!({"label": label}), target)?;
            Ok(())
        }
        Block::Move { block, to } => {
            let mover = named_block(session, block);
            let moving = session.call_on("block.inspect", Value::Null, mover)?;
            let position = match place(&to) {
                Ok(index) => index,
                Err((other, after)) => {
                    let named = named_block(session, Some(other));
                    let other = session.call_on("block.inspect", Value::Null, named)?;
                    if other["window"] != moving["window"] {
                        return Err(Stop::Usage(format!(
                            "{} is in another window: move blocks between windows with \
                             plays do block.move_to_window",
                            show::text(&other["id"])
                        )));
                    }
                    beside(&moving, &other, after, "block")?
                }
            };
            let id = Target {
                window: None,
                block: Some(show::text(&moving["id"])),
            };
            session.call_on("block.move", json!({"position": position}), id)?;
            Ok(())
        }
    }
}

/// Run `command` on the block `block` names, or `-b`'s.
fn run_on_block(session: &mut Session, command: &str, block: Option<String>) -> Done {
    let target = named_block(session, block);
    session.call_on(command, Value::Null, target)?;
    Ok(())
}

/// The window `id` is.
fn in_window(id: &Value) -> Target {
    Target {
        window: Some(show::text(id)),
        block: None,
    }
}

/// The window `name` names, or `-w`'s.
fn named_window(session: &Session, name: Option<String>) -> Target {
    Target {
        window: name.or_else(|| session.options.window.clone()),
        block: None,
    }
}

/// The block `name` names, in `-w`'s window if named, or `-b`'s.
fn named_block(session: &Session, name: Option<String>) -> Target {
    Target {
        window: session.options.window.clone(),
        block: name.or_else(|| session.options.block.clone()),
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

/// The blocks `names` name, by id, found before any changes: or `-b`'s,
/// with none.
fn block_ids(session: &mut Session, names: Vec<String>) -> Result<Vec<Target>, Stop> {
    if names.is_empty() {
        return Ok(vec![named_block(session, None)]);
    }
    names
        .into_iter()
        .map(|name| {
            let inspected = session.call_on(
                "block.inspect",
                Value::Null,
                named_block(session, Some(name)),
            )?;
            Ok(Target {
                window: None,
                block: Some(show::text(&inspected["id"])),
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

/// What a block is called: its label, or its file's name.
fn name(block: &Value) -> String {
    match &block["label"] {
        Value::Null => show::cell(&block["title"]),
        label => show::text(label),
    }
}

/// What a block is doing: opening its file, stopped on an error, playing
/// or paused.
fn state(block: &Value) -> &'static str {
    if !block["error"].is_null() {
        "failed"
    } else if block["opened"] == false {
        "opening"
    } else if block["playing"] == true {
        "playing"
    } else {
        "paused"
    }
}

#[cfg(test)]
#[path = "tests/windows.rs"]
mod tests;
