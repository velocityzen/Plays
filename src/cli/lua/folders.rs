//! `plays.ls`: what's in a folder, which Lua's own library can't list.

use std::fs;
use std::path::Path;

use mlua::{Lua, Table};
use serde_json::{Value, json};

use super::{Answer, to_lua};

/// `plays.ls`, under `plays`.
pub fn add(lua: &Lua, plays: &Table) -> mlua::Result<()> {
    plays.set("ls", lua.create_function(ls)?)
}

/// `plays.ls(folder)`: what's in `folder`, as `listing` says; or nil and
/// why it can't be read.
fn ls(lua: &Lua, folder: String) -> mlua::Result<Answer> {
    Ok(Answer(match listing(Path::new(&folder)) {
        Ok(listed) => Ok(to_lua(lua, &listed)?),
        Err(why) => Err(why),
    }))
}

/// Everything in `folder`, hidden too, by name: each `{name, path, type}`,
/// its full path, and its type "file", "folder" or "other", a link's what
/// it points at. A relative `folder` is from where the script runs.
fn listing(folder: &Path) -> Result<Value, String> {
    let unreadable = |err: std::io::Error| format!("{}: {err}", folder.display());
    let folder = std::path::absolute(folder).map_err(unreadable)?;
    let mut entries: Vec<_> = fs::read_dir(&folder)
        .map_err(unreadable)?
        .flatten()
        .map(|entry| (entry.file_name(), entry.path()))
        .collect();
    entries.sort();
    let listed = entries.into_iter().map(|(name, path)| {
        let kind = match fs::metadata(&path) {
            Ok(found) if found.is_dir() => "folder",
            Ok(found) if found.is_file() => "file",
            _ => "other",
        };
        json!({
            "name": name.to_string_lossy(),
            "path": path.to_string_lossy(),
            "type": kind,
        })
    });
    Ok(listed.collect())
}

#[cfg(test)]
#[path = "tests/folders.rs"]
mod tests;
