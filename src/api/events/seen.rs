//! What a look at the app saw, and the events between one look and the
//! next: what changed, told once, in order.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::Event;
use crate::api::Id;
use crate::settings::OverVideo;

/// The kinds of track, as a block's `tracks` has them and events name them.
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
    /// The panels its blocks show over their videos, as those opened in it
    /// start out.
    pub over_video: OverVideo,
    /// Its blocks, in the order its grid shows them.
    pub blocks: Vec<BlockSeen>,
    /// The block filling the grid, if one does.
    pub maximized: Option<Id>,
    /// The block keys go to.
    pub active: Option<Id>,
}

/// A block, and how it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockSeen {
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
    /// It plays, and has moved since it was set to.
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
    /// It plays its file again from the start at its end.
    pub looping: bool,
    /// It shows what its file is and how it plays, over its picture.
    pub console: bool,
    /// The panels it shows over its video.
    pub over_video: OverVideo,
}

/// The events of panels over the video showing or hiding, each `[on,
/// off]`: of any showing, where none did, or none, then of each panel, in
/// the order of `OverVideo::PANELS`.
pub type PanelEvents = [[&'static str; 2]; 5];

/// The panels' events of a window.
const WINDOW_PANELS: PanelEvents = [
    ["window.ui.on", "window.ui.off"],
    ["window.controls.on", "window.controls.off"],
    ["window.top_bar.on", "window.top_bar.off"],
    ["window.track_switcher.on", "window.track_switcher.off"],
    ["window.notes.on", "window.notes.off"],
];

/// The panels' events of a block.
const BLOCK_PANELS: PanelEvents = [
    ["block.ui.on", "block.ui.off"],
    ["block.controls.on", "block.controls.off"],
    ["block.top_bar.on", "block.top_bar.off"],
    ["block.track_switcher.on", "block.track_switcher.off"],
    ["block.notes.on", "block.notes.off"],
];

/// Of `kinds`, the events of the panels `new` shows, against `old`.
pub fn panels_switched(old: OverVideo, new: OverVideo, kinds: &PanelEvents) -> Vec<&'static str> {
    let shown = old.shown().into_iter().zip(new.shown());
    std::iter::once((old.any(), new.any()))
        .chain(shown)
        .zip(kinds)
        .filter_map(|((was, is), &kind)| switched(was, is, kind))
        .collect()
}

/// Of `[on, off]`, what's told of a feature that `was` on, or not, and
/// `is`: none if it is as it was.
pub fn switched(was: bool, is: bool, [on, off]: [&'static str; 2]) -> Option<&'static str> {
    (was != is).then_some(if is { on } else { off })
}

impl BlockSeen {
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
    let frame = frame_rate
        .filter(|&fps| fps > 0.0)
        .map(|fps| frame_at(time, fps));
    json!({"position": position, "time": time, "frame": frame})
}

/// The number of the frame showing at `time` seconds, from 0, at `fps`
/// frames a second: a time before the start counts as the first frame's.
pub fn frame_at(time: f64, fps: f64) -> u64 {
    (time * fps + FRAME_SLACK).floor() as u64
}

/// The events between `old` and `new`, in the order they're told: the
/// app's, the windows opened, then each window's in turn (its own, then its
/// blocks', then its layout's), the windows closed, and last the window in
/// front. Positions aren't among them: they're told on a clock of their
/// own.
pub fn changes(old: &Seen, new: &Seen) -> Vec<Event> {
    let mut events = Vec::new();
    if old.outputs != new.outputs {
        events.push(Event::app(
            "output.devices_changed",
            json!({"outputs": new.outputs}),
        ));
    }
    if old.route != new.route {
        events.push(Event::app(
            "output.route_changed",
            json!({"route": new.route}),
        ));
    }
    if old.devices != new.devices {
        events.push(Event::app(
            "cast.devices_changed",
            json!({"devices": new.devices}),
        ));
    }
    // Where each block was, to tell one moved from one closed.
    let was: HashMap<Id, (&WindowSeen, &BlockSeen)> = old
        .windows
        .iter()
        .flat_map(|window| {
            window
                .blocks
                .iter()
                .map(move |block| (block.id, (window, block)))
        })
        .collect();
    let is: HashSet<Id> = new
        .windows
        .iter()
        .flat_map(|window| window.blocks.iter().map(|block| block.id))
        .collect();
    let before = |id: Id| old.windows.iter().find(|window| window.id == id);
    for window in &new.windows {
        if before(window.id).is_none() {
            events.push(Event::of_window(
                "window.created",
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
        for block in window.blocks.iter().filter(|block| !is.contains(&block.id)) {
            events.push(Event::block("block.closed", window, block, json!({})));
        }
        events.push(Event::of_window("window.closed", window, json!({})));
    }
    if old.front != new.front {
        let front = new
            .windows
            .iter()
            .find(|window| Some(window.id) == new.front);
        let mut event = Event::app(
            "window.active_changed",
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
/// opened): its label and full screen; the blocks that came, from nowhere or
/// from elsewhere in `was`, and those gone, not to elsewhere in `is`; its
/// layout and the block keys go to; then how each block plays.
fn window_changes(
    before: Option<&WindowSeen>,
    window: &WindowSeen,
    was: &HashMap<Id, (&WindowSeen, &BlockSeen)>,
    is: &HashSet<Id>,
    events: &mut Vec<Event>,
) {
    // One just opened is as its `window.created` said.
    let opened = WindowSeen {
        full_screen: false,
        blocks: Vec::new(),
        maximized: None,
        active: None,
        ..window.clone()
    };
    let before = before.unwrap_or(&opened);
    let id = window.id;
    if before.label != window.label {
        events.push(Event::of_window(
            "window.label_changed",
            window,
            json!({"label": window.label}),
        ));
    }
    let full_screen = ["window.full_screen.on", "window.full_screen.off"];
    let panels = panels_switched(before.over_video, window.over_video, &WINDOW_PANELS);
    let switches = switched(before.full_screen, window.full_screen, full_screen)
        .into_iter()
        .chain(panels);
    events.extend(switches.map(|kind| Event::window(kind, window, json!({}))));
    let mut olds = Vec::with_capacity(window.blocks.len());
    for block in &window.blocks {
        let old = match was.get(&block.id) {
            None => {
                events.push(Event::block(
                    "block.created",
                    window,
                    block,
                    json!({"label": block.label}),
                ));
                None
            }
            Some(&(from, old)) => {
                if from.id != id {
                    let mut moved = Event::block(
                        "block.moved",
                        window,
                        block,
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
    for block in before.blocks.iter().filter(|block| !is.contains(&block.id)) {
        events.push(Event::block("block.closed", window, block, json!({})));
    }
    let ids = |window: &WindowSeen| {
        window
            .blocks
            .iter()
            .map(|block| block.id)
            .collect::<Vec<_>>()
    };
    let blocks = ids(window);
    if ids(before) != blocks || before.maximized != window.maximized {
        let blocks: Vec<String> = blocks.iter().map(Id::to_string).collect();
        events.push(Event::window(
            "window.layout_changed",
            window,
            json!({
                "blocks": blocks,
                "maximized": window.maximized.map(|id| id.to_string()),
            }),
        ));
    }
    if before.active != window.active {
        let active = window
            .blocks
            .iter()
            .find(|block| Some(block.id) == window.active);
        events.push(Event::window(
            "block.active_changed",
            window,
            json!({
                "block_id": window.active.map(|id| id.to_string()),
                "block_title": active.map(|block| &block.title),
            }),
        ));
    }
    for (block, old) in window.blocks.iter().zip(olds) {
        block_changes(old, block, window, events);
    }
}

/// What changed in `block`, in `window`, `old` as it was (none if it's
/// new): its label, the device it casts to, the file opening or failing,
/// then how that file plays.
fn block_changes(
    old: Option<&BlockSeen>,
    block: &BlockSeen,
    window: &WindowSeen,
    events: &mut Vec<Event>,
) {
    let mut tell = |kind, payload| events.push(Event::block(kind, window, block, payload));
    if let Some(old) = old
        && old.label != block.label
    {
        tell("block.label_changed", json!({"label": block.label}));
    }
    if let Some(old) = old {
        let console = ["block.console.on", "block.console.off"];
        let looping = ["playback.loop.on", "playback.loop.off"];
        let switches = panels_switched(old.over_video, block.over_video, &BLOCK_PANELS)
            .into_iter()
            .chain(switched(old.console, block.console, console))
            .chain(switched(old.looping, block.looping, looping));
        for kind in switches {
            tell(kind, json!({}));
        }
    }
    // A cast goes on from one file to the next.
    let cast = old.and_then(|old| old.casting.as_ref());
    if cast != block.casting.as_ref() {
        if let Some(device) = cast {
            tell("cast.ended", json!({"device": device}));
        }
        if let Some(device) = &block.casting {
            tell("cast.started", json!({"device": device}));
        }
    }
    // As it was with this file, if it was this file.
    let file = old.filter(|old| old.opening == block.opening);
    if let Some(opened) = &block.opened
        && file.is_none_or(|old| old.opened.is_none())
    {
        tell("playback.opened", opened.clone());
    }
    if let Some(error) = &block.failed
        && file.is_none_or(|old| old.failed.is_none())
    {
        // Once it has opened, it couldn't play on.
        let kind = if block.opened.is_some() {
            "playback.failed"
        } else {
            "playback.open_failed"
        };
        tell(kind, json!({"error": error}));
    }
    if block.opened.is_some() {
        playing_changes(file.filter(|old| old.opened.is_some()), block, &mut tell);
    }
    let picture_in_picture = [
        "block.picture_in_picture.on",
        "block.picture_in_picture.off",
    ];
    let was = old.is_some_and(|old| old.picture_in_picture);
    if let Some(kind) = switched(was, block.picture_in_picture, picture_in_picture) {
        tell(kind, json!({}));
    }
}

/// How the file `block` has open plays now, against `old` (none if it just
/// opened, which starts out paused, as its `playback.opened` says the rest).
fn playing_changes(
    old: Option<&BlockSeen>,
    block: &BlockSeen,
    tell: &mut impl FnMut(&'static str, Value),
) {
    let opened = BlockSeen {
        playing: false,
        ended: false,
        ..block.clone()
    };
    let old = old.unwrap_or(&opened);
    if old.seeks != block.seeks {
        tell("playback.seeked", block.playhead(block.sought));
    }
    if block.ended && !old.ended {
        tell("playback.ended", block.playhead(block.position));
    } else if old.playing != block.playing {
        let kind = if block.playing {
            "playback.playing"
        } else {
            "playback.paused"
        };
        tell(kind, block.playhead(block.position));
    }
    if old.volume != block.volume {
        tell("audio.volume_changed", json!({"volume": block.volume}));
    }
    if let Some(kind) = switched(old.muted, block.muted, ["audio.mute.on", "audio.mute.off"]) {
        tell(kind, json!({}));
    }
    for ((kind, was), track) in TRACKS.iter().zip(old.tracks).zip(block.tracks) {
        if was != track {
            tell(
                "playback.track_changed",
                json!({"kind": kind, "track": track}),
            );
        }
    }
    let [.., subtitles_were] = old.tracks;
    let [.., subtitles] = block.tracks;
    let shown = ["subtitles.on", "subtitles.off"];
    if let Some(kind) = switched(subtitles_were.is_some(), subtitles.is_some(), shown) {
        tell(kind, json!({}));
    }
    if old.chapter != block.chapter {
        tell(
            "playback.chapter_changed",
            json!({"chapter": block.chapter}),
        );
    }
}

#[cfg(test)]
#[path = "tests/seen.rs"]
mod tests;
