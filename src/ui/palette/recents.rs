//! The commands run from the palette lately, the latest first, which it
//! lists first. They're kept in the app's folder, for the next launch.

use std::fs;

use gpui::{App, Global};

use crate::files::{self, Folder};

/// How many are kept.
pub const KEPT: usize = 8;

/// The file they're kept in.
const FILE: &str = "recent-commands.json";

/// The ids of the commands run from the palette lately, the latest first.
#[derive(Default)]
struct Recents(Vec<String>);
impl Global for Recents {}

/// Those run lately, the latest first, read from their file the first
/// time.
pub fn list(cx: &mut App) -> Vec<String> {
    if !cx.has_global::<Recents>() {
        let kept = load(cx);
        cx.set_global(Recents(kept));
    }
    cx.global::<Recents>().0.clone()
}

/// Note that `command` was run from the palette, and keep it.
pub fn note(command: &str, cx: &mut App) {
    let mut recents = list(cx);
    recents.retain(|id| id != command);
    recents.insert(0, command.to_owned());
    recents.truncate(KEPT);
    save(&recents, cx);
    cx.set_global(Recents(recents));
}

fn load(cx: &App) -> Vec<String> {
    let Some(path) = Folder::of(cx).file(FILE) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&text)
        .inspect_err(|err| log::warn!("ignoring {}: {err}", path.display()))
        .map(|mut recents| {
            recents.truncate(KEPT);
            recents
        })
        .unwrap_or_default()
}

fn save(recents: &[String], cx: &App) {
    let Some(path) = Folder::of(cx).file(FILE) else {
        return;
    };
    let result = serde_json::to_vec_pretty(recents)
        .map_err(std::io::Error::other)
        .and_then(|text| files::write_whole(&path, &text));
    if let Err(err) = result {
        log::warn!("could not save {}: {err}", path.display());
    }
}

/// Forget them, as the next launch starts: they're read from their file
/// again.
#[cfg(test)]
pub fn forget(cx: &mut App) {
    if cx.has_global::<Recents>() {
        cx.remove_global::<Recents>();
    }
}
