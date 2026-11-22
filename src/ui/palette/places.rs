//! Go to…: the places the palette goes to, one list of the windows, the
//! tiles in them, the media files in the front tile's folder and the files
//! played lately. Each runs a command: Go to Window or Go to Tile, Play
//! File Instead in the front tile, or Open Paths or URLs in the window.

use std::path::{Path, PathBuf};

use gpui::{App, SharedString};
use serde_json::{Value, json};

use super::Row;
use super::search::{self, Haystack};
use crate::api::{self, Invocation, Source, Target};
use crate::files;
use crate::recents;

/// What a place is, which its row says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Window,
    Tile,
    /// A media file in the front tile's folder.
    InFolder,
    /// A file played lately.
    Recent,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Window => "Window",
            Self::Tile => "Tile",
            Self::InFolder => "In Folder",
            Self::Recent => "Recent",
        }
    }
}

/// A place to go to, and the command that goes there.
#[derive(Clone, Debug)]
pub struct Place {
    pub kind: Kind,
    pub label: SharedString,
    /// Where it is: a tile's window, a recent file's folder.
    pub detail: Option<SharedString>,
    pub run: Invocation,
    /// Where it is already: this window, its tile in front, the file it
    /// plays.
    pub current: bool,
}

impl Place {
    fn new(kind: Kind, label: impl Into<SharedString>, run: Invocation) -> Self {
        Self {
            kind,
            label: label.into(),
            detail: None,
            run,
            current: false,
        }
    }

    fn detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail.map(Into::into);
        self
    }

    fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
}

/// `command` from the palette, with `target`.
fn invocation(command: &str, args: Value, target: Target) -> Invocation {
    let mut invocation = Invocation::new(command)
        .with_args(args)
        .from(Source::Palette);
    invocation.target = target;
    invocation
}

/// Everywhere to go from `window` (a target naming one): the windows, in
/// their order, the tiles in each, the media files in its front tile's
/// folder, once it has been looked through, and the files played lately,
/// the latest first.
pub fn places(window: &Target, cx: &mut App) -> Vec<Place> {
    let windows = query("window.ls", Target::default(), cx);
    let windows = windows.as_array().map_or(&[][..], Vec::as_slice);
    let mut places = Vec::new();
    for listed in windows {
        let Some(id) = listed["id"].as_str() else {
            continue;
        };
        let here = window.window.as_deref() == Some(id);
        let tiles = listed["tiles"].as_u64().unwrap_or_default();
        let files = match tiles {
            0 => "Empty".to_owned(),
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        places.push(
            Place::new(
                Kind::Window,
                window_name(listed),
                invocation("window.focus", Value::Null, named(id, None)),
            )
            .detail(Some(files))
            .current(here),
        );
    }
    for listed in windows {
        let Some(id) = listed["id"].as_str() else {
            continue;
        };
        let here = window.window.as_deref() == Some(id);
        // Which window a tile is in, if there's more than one.
        let within = (windows.len() > 1).then(|| window_name(listed));
        let tiles = query("tile.ls", named(id, None), cx);
        for tile in tiles.as_array().into_iter().flatten() {
            let Some(tile_id) = tile["id"].as_str() else {
                continue;
            };
            let label = text(&tile["label"])
                .or_else(|| text(&tile["title"]))
                .unwrap_or("Untitled");
            places.push(
                Place::new(
                    Kind::Tile,
                    label.to_owned(),
                    invocation("tile.focus", Value::Null, named(id, Some(tile_id))),
                )
                .detail(within.clone())
                .current(here && tile["front"] == true),
            );
        }
    }
    places.extend(in_folder(window, cx));
    places.extend(recent(window, cx));
    places
}

/// The media files in `window`'s front tile's folder, by name, and the
/// one it plays; none until it has been looked through, or for a stream.
pub fn folder_files(window: &Target, cx: &App) -> (Vec<PathBuf>, Option<PathBuf>) {
    api::read_tile(window, cx, |player, _| {
        (
            player.folder_files().map(<[_]>::to_vec).unwrap_or_default(),
            player.file().map(Path::to_owned),
        )
    })
    .unwrap_or_default()
}

/// The media files in `window`'s front tile's folder, the one it plays
/// picked.
fn in_folder(window: &Target, cx: &App) -> Vec<Place> {
    let (files, playing) = folder_files(window, cx);
    files
        .into_iter()
        .map(|file| {
            let current = playing.as_ref() == Some(&file);
            Place::new(
                Kind::InFolder,
                files::name(&file),
                invocation("playback.play_file", json!({"path": file}), window.clone()),
            )
            .current(current)
        })
        .collect()
}

/// The files played lately, the latest first, each with its folder, to
/// open in `window`.
fn recent(window: &Target, cx: &mut App) -> Vec<Place> {
    recents::FILES
        .list(cx)
        .into_iter()
        .map(|file| {
            let path = Path::new(&file);
            let folder = path.parent().map(files::shown);
            Place::new(
                Kind::Recent,
                files::name(path),
                invocation("app.open", json!({"urls": [file]}), window.clone()),
            )
            .detail(folder)
        })
        .collect()
}

/// The places `query` finds, by their names, kinds or where they are,
/// best first; with nothing typed, all of them, in their order.
pub fn order(query: &str, places: &[Place]) -> Vec<Row> {
    let mut found: Vec<(i32, Row)> = places
        .iter()
        .enumerate()
        .filter_map(|(index, place)| {
            let detail = place.detail.as_ref().map(SharedString::as_ref);
            let keywords: Vec<&str> = detail.into_iter().collect();
            let haystack = Haystack {
                title: &place.label,
                group: place.kind.name(),
                keywords: &keywords,
            };
            let found = search::rank(query, &haystack)?;
            Some((
                found.score,
                Row {
                    index,
                    at: found.at,
                },
            ))
        })
        .collect();
    if !query.trim().is_empty() {
        found.sort_by_key(|(score, row)| (std::cmp::Reverse(*score), row.index));
    }
    found.into_iter().map(|(_, row)| row).collect()
}

/// What `command` hands back from the palette for `target`, or null.
fn query(command: &str, target: Target, cx: &mut App) -> Value {
    api::run(invocation(command, Value::Null, target), cx).unwrap_or_default()
}

/// The target naming the window `window`, and its tile `tile`.
fn named(window: &str, tile: Option<&str>) -> Target {
    Target {
        window: Some(window.to_owned()),
        tile: tile.map(str::to_owned),
    }
}

/// What a window is called: its label, or its number.
pub fn window_name(window: &Value) -> String {
    text(&window["label"]).map_or_else(
        || format!("Window {}", window["number"].as_u64().unwrap_or_default()),
        str::to_owned,
    )
}

pub fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

#[cfg(test)]
#[path = "tests/places.rs"]
mod tests;
