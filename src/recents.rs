//! What was used lately, the latest first, each once: the commands run
//! from the palette, which it lists first, and the files played, which
//! File → Open Recent and Go to… offer. Each list is kept in a file of its
//! own in the app's folder, for the next launch.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use gpui::{App, Global};

use crate::files::{self, Folder};

/// A list of what was used lately, and the file it's kept in.
pub struct Recents {
    file: &'static str,
    /// How many it keeps.
    pub kept: usize,
}

/// The ids of the commands run from the palette.
pub const COMMANDS: Recents = Recents {
    file: "recent-commands.json",
    kept: 8,
};

/// The full paths of the files played.
pub const FILES: Recents = Recents {
    file: "recent-files.json",
    kept: 20,
};

/// Each list read so far, by its file.
#[derive(Default)]
struct Kept(HashMap<&'static str, Vec<String>>);
impl Global for Kept {}

impl Recents {
    /// What was used lately, the latest first, read from its file the
    /// first time.
    pub fn list(&self, cx: &mut App) -> Vec<String> {
        if let Some(list) = cx
            .try_global::<Kept>()
            .and_then(|kept| kept.0.get(self.file))
        {
            return list.clone();
        }
        let list = self.load(cx);
        cx.default_global::<Kept>()
            .0
            .insert(self.file, list.clone());
        list
    }

    /// Note that `item` was used, first now, and keep it.
    pub fn note(&self, item: &str, cx: &mut App) {
        let mut list = self.list(cx);
        list.retain(|other| other != item);
        list.insert(0, item.to_owned());
        list.truncate(self.kept);
        self.keep(list, cx);
    }

    /// Forget them all, the file too.
    pub fn clear(&self, cx: &mut App) {
        self.keep(Vec::new(), cx);
    }

    fn keep(&self, list: Vec<String>, cx: &mut App) {
        self.save(&list, cx);
        cx.default_global::<Kept>().0.insert(self.file, list);
    }

    fn load(&self, cx: &App) -> Vec<String> {
        let Some(path) = Folder::of(cx).file(self.file) else {
            return Vec::new();
        };
        let Ok(text) = fs::read_to_string(&path) else {
            return Vec::new();
        };
        serde_json::from_str::<Vec<String>>(&text)
            .inspect_err(|err| log::warn!("ignoring {}: {err}", path.display()))
            .map(|mut list| {
                list.truncate(self.kept);
                list
            })
            .unwrap_or_default()
    }

    /// Save `list` in its file, soon, off the UI thread.
    fn save(&self, list: &[String], cx: &mut App) {
        let Some(path) = Folder::of(cx).file(self.file) else {
            return;
        };
        match serde_json::to_vec_pretty(list) {
            Ok(text) => files::write_soon(path, text, cx),
            Err(err) => log::warn!("could not save {}: {err}", path.display()),
        }
    }
}

/// Note `file` among the files played lately, by its full path.
pub fn note_file(file: &Path, cx: &mut App) {
    let path = std::path::absolute(file).unwrap_or_else(|_| file.to_owned());
    FILES.note(&path.to_string_lossy(), cx);
}

/// Call `changed` whenever a list changes.
pub fn observe(cx: &mut App, changed: impl FnMut(&mut App) + 'static) {
    cx.observe_global::<Kept>(changed).detach();
}

/// Forget every list, as the next launch starts: they're read from their
/// files again.
#[cfg(test)]
pub fn forget(cx: &mut App) {
    if cx.has_global::<Kept>() {
        cx.remove_global::<Kept>();
    }
}

#[cfg(test)]
#[path = "tests/recents.rs"]
mod tests;
