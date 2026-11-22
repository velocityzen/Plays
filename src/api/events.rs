//! Events: what changes in the app, its windows and their tiles, told to
//! whoever listens (`listen`), as the command line does through the
//! socket. Each is a JSON object, `{type, payload}`; the payload of one
//! about a window or a tile has its `window_id` and `window_title`, and
//! `tile_id` and `tile_title`: each id with the title it shows.
//!
//! A listener hears the app's events (`Scope::App`: windows opening,
//! closing, renamed and coming to the front, outputs, devices, settings,
//! keys, commands run, and the socket's clients connecting and leaving),
//! one window's (`Scope::Window`: its own, its tiles' and how they play),
//! or all of them. Where a file plays is told as how far through it is,
//! from 0 to 1, in seconds and by frame (`playhead`). A tile's position
//! is told only to those who ask for it, at most once a second. Readers
//! ignore events and fields they don't know, so later versions can add
//! them.
//!
//! What changed is found by looking (`look`): at what the app, its windows
//! and tiles are like, against what the last look saw (`seen`), once each
//! command has run and every `LOOK_EVERY` while anyone listens. So each change
//! is told once, whatever made it: a command, a click, or the file itself,
//! opening or ending. Settings, which change where they're kept, are told
//! as they change. Nothing is looked at while no one listens. As the app
//! quits (`quit`), each window still open is told closing, then `quit`,
//! which every listener hears, one window's too.

mod seen;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{App, Global};
use serde::Serialize;
use serde_json::{Value, json};

pub use seen::{Seen, TRACKS, TileSeen, WindowSeen, playhead};

use super::commands::{self, cast, output};
use super::{Id, Invocation, target};
use crate::hotkeys;
use crate::settings::{Settings, fields};

/// How often the app is looked at while anyone listens, for what changed
/// that no command did: a file opening or ending, a tile the mouse goes
/// to, a window coming to the front.
pub const LOOK_EVERY: Duration = Duration::from_millis(250);

/// How often a tile's position is told, at most, to those who ask.
pub const POSITION_EVERY: Duration = Duration::from_secs(1);

/// Something that changed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub payload: Value,
    /// Who hears it.
    #[serde(skip)]
    reach: Reach,
}

/// Who hears an event: those listening to the app's, those listening to
/// one of `windows`, and those listening to all; or `everyone`.
#[derive(Clone, Debug, Default, PartialEq)]
struct Reach {
    app: bool,
    windows: Vec<Id>,
    everyone: bool,
}

impl Event {
    /// One of the app's.
    fn app(kind: &'static str, payload: Value) -> Self {
        Self {
            kind,
            payload,
            reach: Reach {
                app: true,
                ..Reach::default()
            },
        }
    }

    /// One every listener hears, whatever it listens to: the app quitting.
    fn to_everyone(kind: &'static str, payload: Value) -> Self {
        Self {
            kind,
            payload,
            reach: Reach {
                app: true,
                windows: Vec::new(),
                everyone: true,
            },
        }
    }

    /// One of `window`'s.
    fn window(kind: &'static str, window: &WindowSeen, payload: Value) -> Self {
        Self {
            kind,
            payload: with_ids(payload, window, None),
            reach: Reach {
                windows: vec![window.id],
                ..Reach::default()
            },
        }
    }

    /// One of `window`'s that the app's listeners hear too: its opening,
    /// closing and label, as the windows are the app's.
    fn of_window(kind: &'static str, window: &WindowSeen, payload: Value) -> Self {
        let mut event = Self::window(kind, window, payload);
        event.reach.app = true;
        event
    }

    /// One of `tile`'s, in `window`.
    fn tile(kind: &'static str, window: &WindowSeen, tile: &TileSeen, payload: Value) -> Self {
        Self {
            kind,
            payload: with_ids(payload, window, Some(tile)),
            reach: Reach {
                windows: vec![window.id],
                ..Reach::default()
            },
        }
    }

    /// Whether a listener to `scope` hears it.
    pub fn heard_in(&self, scope: Scope) -> bool {
        match scope {
            Scope::All => true,
            Scope::App => self.reach.app,
            Scope::Window(id) => self.reach.everyone || self.reach.windows.contains(&id),
        }
    }
}

/// `payload` with `window`'s id and title, and `tile`'s, put first.
fn with_ids(payload: Value, window: &WindowSeen, tile: Option<&TileSeen>) -> Value {
    let mut ids = serde_json::Map::new();
    ids.insert("window_id".into(), json!(window.id.to_string()));
    ids.insert("window_title".into(), json!(window.title));
    if let Some(tile) = tile {
        ids.insert("tile_id".into(), json!(tile.id.to_string()));
        ids.insert("tile_title".into(), json!(tile.title));
    }
    if let Value::Object(fields) = payload {
        ids.extend(fields);
    }
    Value::Object(ids)
}

/// Whose events a listener hears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The app's: windows opening, closing, renamed and coming to the
    /// front; outputs, where the sound goes, devices to cast to, settings,
    /// keys, and commands run.
    App,
    /// One window's: its own, its tiles', and how they play.
    Window(Id),
    /// Everything.
    All,
}

/// Someone listening.
struct Listening {
    scope: Scope,
    /// It asked for positions.
    positions: bool,
    sender: UnboundedSender<Event>,
}

impl Listening {
    fn hears(&self, event: &Event) -> bool {
        event.heard_in(self.scope) && (event.kind != "position" || self.positions)
    }
}

/// Who listens, and what was seen last, to tell what changed since.
#[derive(Default)]
struct Events {
    listening: Vec<Listening>,
    /// What the last look saw; none while no one listens.
    seen: Option<Seen>,
    /// The settings as last told.
    settings: Option<Settings>,
    /// When each tile's position was told last, and what it was.
    told: HashMap<Id, (Instant, f64)>,
    /// It looks every `LOOK_EVERY`.
    looking: bool,
}

impl Global for Events {}

/// Tell what settings change, and the keys with them, as they change.
pub fn register(cx: &mut App) {
    cx.observe_global::<Settings>(settings_changed).detach();
}

/// Hear `scope`'s events from now on, and, with `positions`, where each
/// tile is as it plays. Dropping what this hands back stops them.
pub fn listen(scope: Scope, positions: bool, cx: &mut App) -> UnboundedReceiver<Event> {
    let (sender, receiver) = unbounded();
    if !listened(cx) {
        // What changes is told from now on.
        let seen = see(cx);
        let settings = cx.global::<Settings>().clone();
        let events = cx.default_global::<Events>();
        events.seen = Some(seen);
        events.settings = Some(settings);
        if !std::mem::replace(&mut events.looking, true) {
            keep_looking(cx);
        }
    }
    cx.default_global::<Events>().listening.push(Listening {
        scope,
        positions,
        sender,
    });
    receiver
}

/// Whether anyone listens, forgetting those who stopped; when no one does,
/// what was seen is forgotten too.
fn listened(cx: &mut App) -> bool {
    let Some(events) = cx.try_global::<Events>() else {
        return false;
    };
    if events.listening.iter().all(|l| !l.sender.is_closed()) {
        return !events.listening.is_empty();
    }
    let events = cx.global_mut::<Events>();
    events
        .listening
        .retain(|listening| !listening.sender.is_closed());
    if events.listening.is_empty() {
        // Looking stops at its next look.
        *events = Events {
            looking: events.looking,
            ..Events::default()
        };
    }
    !events.listening.is_empty()
}

/// How many listen, forgetting those who stopped: a test goes on once
/// what it runs on another thread does.
#[cfg(test)]
pub fn listeners(cx: &mut App) -> usize {
    if !listened(cx) {
        return 0;
    }
    cx.global::<Events>().listening.len()
}

/// Look every `LOOK_EVERY`, until no one listens.
fn keep_looking(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(LOOK_EVERY).await;
            let listened = cx.update(|cx| {
                look(cx);
                let listened = listened(cx);
                if !listened {
                    cx.global_mut::<Events>().looking = false;
                }
                listened
            });
            if !matches!(listened, Ok(true)) {
                break;
            }
        }
    })
    .detach();
}

/// Tell `event` to those who hear it.
fn tell(event: &Event, cx: &App) {
    let Some(events) = cx.try_global::<Events>() else {
        return;
    };
    for listening in events.listening.iter().filter(|l| l.hears(event)) {
        listening.sender.unbounded_send(event.clone()).ok();
    }
}

/// Look at the app, and tell what changed since it was last looked at.
///
/// Not while a window or a tile is being updated: it reads every one, as
/// `api::run` runs a command.
pub fn look(cx: &mut App) {
    if !listened(cx) {
        return;
    }
    let seen = see(cx);
    let events = cx.global_mut::<Events>();
    let old = events.seen.replace(seen.clone()).unwrap_or_default();
    for event in seen::changes(&old, &seen) {
        tell(&event, cx);
    }
    positions(&seen, cx);
}

/// What the app is like now.
fn see(cx: &mut App) -> Seen {
    let front =
        target::front_window(cx).and_then(|window| Some(target::read(window, cx).ok()?.id()));
    let windows = target::windows(cx)
        .into_iter()
        .filter_map(|window| {
            window
                .update(cx, |workspace, window, cx| workspace.seen(window, cx))
                .ok()
        })
        .collect();
    let outputs = output::listing(cx);
    Seen {
        outputs: outputs["outputs"].clone(),
        route: outputs["route"].clone(),
        devices: cast::found(cx),
        windows,
        front,
    }
}

/// The app quits: what changed since the last look is told, then each
/// window still open closing, its tiles first, as closing it would, then
/// `quit`, which every listener hears. The socket writes them out as it
/// quits (`socket::server`), as nothing else runs then.
pub fn quit(cx: &mut App) {
    if !listened(cx) {
        return;
    }
    look(cx);
    let Some(seen) = cx.global::<Events>().seen.clone() else {
        return;
    };
    let closed = Seen {
        windows: Vec::new(),
        front: None,
        ..seen.clone()
    };
    for event in seen::changes(&seen, &closed) {
        tell(&event, cx);
    }
    tell(&Event::to_everyone("quit", json!({})), cx);
}

/// Tell where each tile that has opened is, to those who asked, at most
/// once every `POSITION_EVERY`, and only once it has moved.
fn positions(seen: &Seen, cx: &mut App) {
    let now = cx.background_executor().now();
    let events = cx.global_mut::<Events>();
    if !events.listening.iter().any(|listening| listening.positions) {
        events.told.clear();
        return;
    }
    let mut told = std::mem::take(&mut events.told);
    told.retain(|id, _| {
        seen.windows
            .iter()
            .any(|window| window.tiles.iter().any(|tile| tile.id == *id))
    });
    for window in &seen.windows {
        for tile in window.tiles.iter().filter(|tile| tile.opened.is_some()) {
            let due = told.get(&tile.id).is_none_or(|&(at, position)| {
                now.duration_since(at) >= POSITION_EVERY && position != tile.position
            });
            if due {
                told.insert(tile.id, (now, tile.position));
                let mut payload = tile.playhead(tile.position);
                payload["duration"] = json!(tile.duration);
                tell(&Event::tile("position", window, tile, payload), cx);
            }
        }
    }
    cx.global_mut::<Events>().told = told;
}

/// `invocation` is about to run, from where it came: told to the app's
/// listeners before anything it changes.
pub fn command_run(invocation: &Invocation, command: &'static str, cx: &App) {
    let mut payload = json!({
        "command": command,
        "source": invocation.source,
    });
    if !invocation.args.is_null() {
        payload["args"] = invocation.args.clone();
    }
    if !invocation.target.is_empty() {
        payload["target"] = json!(invocation.target);
    }
    tell(&Event::app("command_run", payload), cx);
}

/// A client connected to the socket, numbered `client`: told to the
/// app's listeners.
pub fn client_connected(client: u64, cx: &App) {
    let payload = json!({"client_id": client});
    tell(&Event::app("client_connected", payload), cx);
}

/// The client numbered `client` went: told to the app's listeners.
pub fn client_left(client: u64, cx: &App) {
    tell(&Event::app("client_left", json!({"client_id": client})), cx);
}

/// A command has run: look for what it changed once what it set off has
/// settled, as a window it opened is numbered once it's made.
pub fn command_done(cx: &mut App) {
    if listened(cx) {
        cx.defer(look);
    }
}

/// Tell which settings changed, by key, and which commands' keys.
fn settings_changed(cx: &mut App) {
    if !listened(cx) {
        return;
    }
    let settings = cx.global::<Settings>().clone();
    let Some(old) = cx.global_mut::<Events>().settings.replace(settings.clone()) else {
        return;
    };
    let keys = fields::changed(&old, &settings, &commands::settings::protocols(cx));
    if !keys.is_empty() {
        tell(&Event::app("settings_changed", json!({"keys": keys})), cx);
    }
    let commands: Vec<&str> = hotkeys::commands()
        .map(|spec| spec.id)
        .filter(|&id| old.hotkeys.keys(id) != settings.hotkeys.keys(id))
        .collect();
    if !commands.is_empty() {
        tell(
            &Event::app("keymap_changed", json!({"commands": commands})),
            cx,
        );
    }
}

#[cfg(test)]
#[path = "tests/events.rs"]
mod tests;
