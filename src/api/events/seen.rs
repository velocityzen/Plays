//! What a look at the app saw, and the events between one look and the
//! next: what changed, told once, in order.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::Event;
use crate::api::Id;

/// The kinds of track, as a tile's `tracks` has them and events name them.
pub const TRACKS: [&str; 3] = ["video", "audio", "subtitles"];

/// How far short of its frame's number a frame's own time, multiplied
/// back by the frame rate, may fall: a millionth of a frame.
const FRAME_SLACK: f64 = 1e-6;

/// The app, as far as events tell of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    /// The outputs connected, as `output.ls` lists them.
    pub outputs: Value,
    /// Where the app's sound goes, as `output.ls` says.
    pub route: Value,
    /// The devices found to cast to, as `cast.ls` lists them.
    pub devices: Value,
    /// The player windows, in the order ⌘1 to ⌘9 count them.
    pub windows: Vec<WindowSeen>,
    /// The player window in front.
    pub front: Option<Id>,
}

/// A player window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowSeen {
    pub id: Id,
    pub label: Option<String>,
    /// What its title bar shows: its label, or else the app's name.
    pub title: String,
    pub full_screen: bool,
    /// Its tiles, in the order its grid shows them.
    pub tiles: Vec<TileSeen>,
    /// The tile filling the grid, if one does.
    pub maximized: Option<Id>,
    /// The tile keys go to.
    pub active: Option<Id>,
}

/// A tile, and how it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct TileSeen {
    pub id: Id,
    pub label: Option<String>,
    /// What its header shows: its label, or else the title of what it
    /// plays.
    pub title: String,
    /// Counts the files opened in it, to tell one opened in place of
    /// another.
    pub opening: u64,
    /// What it plays, once it has opened: its title, file or URL, and
    /// duration.
    pub opened: Option<Value>,
    /// Why it couldn't open, or play on.
    pub failed: Option<String>,
    pub playing: bool,
    /// Played to its end, and stopped there.
    pub ended: bool,
    /// Counts the seeks, here or on the device it casts to.
    pub seeks: u64,
    /// Where the last seek went.
    pub sought: f64,
    pub position: f64,
    pub duration: Option<f64>,
    /// The video's frames a second; none for sound alone.
    pub frame_rate: Option<f64>,
    /// In percent, as `audio.set_volume` takes it.
    pub volume: f64,
    pub muted: bool,
    /// The tracks picked, numbered from 1 among their kind's, in the order
    /// of `TRACKS`.
    pub tracks: [Option<usize>; 3],
    /// The chapter it's in, from 1.
    pub chapter: Option<usize>,
    /// The device it casts to.
    pub casting: Option<Value>,
    pub picture_in_picture: bool,
}

impl TileSeen {
    /// Where its file is at `time` seconds, as the events of how it plays
    /// tell it (`playhead`).
    pub fn playhead(&self, time: f64) -> Value {
        playhead(time, self.duration, self.frame_rate)
    }
}

/// Where a file is at `time` seconds, as the events of how it plays tell
/// it: how far through it is, from 0 to 1 (none while its `duration`
/// isn't known, as a live stream's), the time, and the number of the frame
/// showing then, from 0 (none without a `frame_rate`, as for sound alone).
pub fn playhead(time: f64, duration: Option<f64>, frame_rate: Option<f64>) -> Value {
    let position = duration
        .filter(|&duration| duration > 0.0)
        .map(|duration| (time / duration).clamp(0.0, 1.0));
    // A time before the start counts as the first frame's.
    let frame = frame_rate
        .filter(|&fps| fps > 0.0)
        .map(|fps| (time * fps + FRAME_SLACK).floor() as u64);
    json!({"position": position, "time": time, "frame": frame})
}

/// The events between `old` and `new`, in the order they're told: the
/// app's, the windows opened, then each window's in turn (its own, then its
/// tiles', then its layout's), the windows closed, and last the window in
/// front. Positions aren't among them: they're told on a clock of their
/// own.
pub fn changes(old: &Seen, new: &Seen) -> Vec<Event> {
    let mut events = Vec::new();
    if old.outputs != new.outputs {
        events.push(Event::app(
            "outputs_changed",
            json!({"outputs": new.outputs}),
        ));
    }
    if old.route != new.route {
        events.push(Event::app("route_changed", json!({"route": new.route})));
    }
    if old.devices != new.devices {
        events.push(Event::app(
            "cast_devices_changed",
            json!({"devices": new.devices}),
        ));
    }
    // Where each tile was, to tell one moved from one closed.
    let was: HashMap<Id, (&WindowSeen, &TileSeen)> = old
        .windows
        .iter()
        .flat_map(|window| {
            window
                .tiles
                .iter()
                .map(move |tile| (tile.id, (window, tile)))
        })
        .collect();
    let is: HashSet<Id> = new
        .windows
        .iter()
        .flat_map(|window| window.tiles.iter().map(|tile| tile.id))
        .collect();
    let before = |id: Id| old.windows.iter().find(|window| window.id == id);
    for window in &new.windows {
        if before(window.id).is_none() {
            events.push(Event::of_window(
                "window_created",
                window,
                json!({"label": window.label}),
            ));
        }
    }
    for window in &new.windows {
        window_changes(before(window.id), window, &was, &is, &mut events);
    }
    for window in &old.windows {
        if new.windows.iter().any(|open| open.id == window.id) {
            continue;
        }
        for tile in window.tiles.iter().filter(|tile| !is.contains(&tile.id)) {
            events.push(Event::tile("tile_closed", window, tile, json!({})));
        }
        events.push(Event::of_window("window_closed", window, json!({})));
    }
    if old.front != new.front {
        let front = new
            .windows
            .iter()
            .find(|window| Some(window.id) == new.front);
        let mut event = Event::app(
            "active_window_changed",
            json!({
                "window_id": new.front.map(|id| id.to_string()),
                "window_title": front.map(|window| &window.title),
            }),
        );
        event.reach.windows.extend(new.front);
        events.push(event);
    }
    events
}

/// What changed in `window`, `before` as it was (none if it just
/// opened): its label and full screen; the tiles that came, from nowhere or
/// from elsewhere in `was`, and those gone, not to elsewhere in `is`; its
/// layout and the tile keys go to; then how each tile plays.
fn window_changes(
    before: Option<&WindowSeen>,
    window: &WindowSeen,
    was: &HashMap<Id, (&WindowSeen, &TileSeen)>,
    is: &HashSet<Id>,
    events: &mut Vec<Event>,
) {
    // One just opened is as its `window_created` said.
    let opened = WindowSeen {
        full_screen: false,
        tiles: Vec::new(),
        maximized: None,
        active: None,
        ..window.clone()
    };
    let before = before.unwrap_or(&opened);
    let id = window.id;
    if before.label != window.label {
        events.push(Event::of_window(
            "window_label_changed",
            window,
            json!({"label": window.label}),
        ));
    }
    if before.full_screen != window.full_screen {
        events.push(Event::window(
            "full_screen_changed",
            window,
            json!({"full_screen": window.full_screen}),
        ));
    }
    let mut olds = Vec::with_capacity(window.tiles.len());
    for tile in &window.tiles {
        let old = match was.get(&tile.id) {
            None => {
                events.push(Event::tile(
                    "tile_created",
                    window,
                    tile,
                    json!({"label": tile.label}),
                ));
                None
            }
            Some(&(from, old)) => {
                if from.id != id {
                    let mut moved = Event::tile(
                        "tile_moved",
                        window,
                        tile,
                        json!({
                            "from_window_id": from.id.to_string(),
                            "from_window_title": from.title,
                        }),
                    );
                    moved.reach.windows.push(from.id);
                    events.push(moved);
                }
                Some(old)
            }
        };
        olds.push(old);
    }
    for tile in before.tiles.iter().filter(|tile| !is.contains(&tile.id)) {
        events.push(Event::tile("tile_closed", window, tile, json!({})));
    }
    let ids = |window: &WindowSeen| window.tiles.iter().map(|tile| tile.id).collect::<Vec<_>>();
    let tiles = ids(window);
    if ids(before) != tiles || before.maximized != window.maximized {
        let tiles: Vec<String> = tiles.iter().map(Id::to_string).collect();
        events.push(Event::window(
            "window_layout_changed",
            window,
            json!({
                "tiles": tiles,
                "maximized": window.maximized.map(|id| id.to_string()),
            }),
        ));
    }
    if before.active != window.active {
        let active = window
            .tiles
            .iter()
            .find(|tile| Some(tile.id) == window.active);
        events.push(Event::window(
            "active_tile_changed",
            window,
            json!({
                "tile_id": window.active.map(|id| id.to_string()),
                "tile_title": active.map(|tile| &tile.title),
            }),
        ));
    }
    for (tile, old) in window.tiles.iter().zip(olds) {
        tile_changes(old, tile, window, events);
    }
}

/// What changed in `tile`, in `window`, `old` as it was (none if it's
/// new): its label, the device it casts to, the file opening or failing,
/// then how that file plays.
fn tile_changes(
    old: Option<&TileSeen>,
    tile: &TileSeen,
    window: &WindowSeen,
    events: &mut Vec<Event>,
) {
    let mut tell = |kind, payload| events.push(Event::tile(kind, window, tile, payload));
    if let Some(old) = old
        && old.label != tile.label
    {
        tell("tile_label_changed", json!({"label": tile.label}));
    }
    // A cast goes on from one file to the next.
    let cast = old.and_then(|old| old.casting.as_ref());
    if cast != tile.casting.as_ref() {
        if let Some(device) = cast {
            tell("cast_ended", json!({"device": device}));
        }
        if let Some(device) = &tile.casting {
            tell("cast_started", json!({"device": device}));
        }
    }
    // As it was with this file, if it was this file.
    let file = old.filter(|old| old.opening == tile.opening);
    if let Some(opened) = &tile.opened
        && file.is_none_or(|old| old.opened.is_none())
    {
        tell("opened", opened.clone());
    }
    if let Some(error) = &tile.failed
        && file.is_none_or(|old| old.failed.is_none())
    {
        // Once it has opened, it couldn't play on.
        let kind = if tile.opened.is_some() {
            "failed"
        } else {
            "open_failed"
        };
        tell(kind, json!({"error": error}));
    }
    if tile.opened.is_some() {
        playing_changes(file.filter(|old| old.opened.is_some()), tile, &mut tell);
    }
    if old.is_some_and(|old| old.picture_in_picture) != tile.picture_in_picture {
        tell(
            "picture_in_picture_changed",
            json!({"picture_in_picture": tile.picture_in_picture}),
        );
    }
}

/// How the file `tile` has open plays now, against `old` (none if it just
/// opened, which starts out paused, as its `opened` says the rest).
fn playing_changes(
    old: Option<&TileSeen>,
    tile: &TileSeen,
    tell: &mut impl FnMut(&'static str, Value),
) {
    let opened = TileSeen {
        playing: false,
        ended: false,
        ..tile.clone()
    };
    let old = old.unwrap_or(&opened);
    if old.seeks != tile.seeks {
        tell("seeked", tile.playhead(tile.sought));
    }
    if tile.ended && !old.ended {
        tell("ended", tile.playhead(tile.position));
    } else if old.playing != tile.playing {
        let kind = if tile.playing { "playing" } else { "paused" };
        tell(kind, tile.playhead(tile.position));
    }
    if (old.volume, old.muted) != (tile.volume, tile.muted) {
        tell(
            "volume_changed",
            json!({"volume": tile.volume, "muted": tile.muted}),
        );
    }
    for ((kind, was), track) in TRACKS.iter().zip(old.tracks).zip(tile.tracks) {
        if was != track {
            tell("track_changed", json!({"kind": kind, "track": track}));
        }
    }
    if old.chapter != tile.chapter {
        tell("chapter_changed", json!({"chapter": tile.chapter}));
    }
}

#[cfg(test)]
#[path = "tests/seen.rs"]
mod tests;
