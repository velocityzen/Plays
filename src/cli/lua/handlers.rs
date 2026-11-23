//! `plays.on`: handlers a script registers as it first runs, for events by
//! their type, each run as one comes once that run is over. One runs at a
//! time, so a wait or a sleep in one holds the others, and a handler's
//! calls act on its event's window and block. The script listens until it
//! stops, `--for` is up, Plays quits, or the window it hears, with `-w`,
//! closes; Ctrl-C, which nothing catches, stops it too.

use std::cell::{Cell, RefCell};
use std::io;
use std::time::{Duration, Instant};

use mlua::{Function, Lua};
use serde_json::Value;

use super::waiting::{Events, POSITION};
use super::{message, to_lua};
use crate::api::Target;
use crate::cli::{Done, Stop, show};

/// How long a script with handlers listens, and whether a handler failing
/// ends it.
#[derive(Clone, Copy, Debug)]
pub struct Listening {
    pub at_most: Option<Duration>,
    pub fail_fast: bool,
}

/// The handlers a script registers, by the type of event each is for, in
/// the order registered.
#[derive(Default)]
pub struct Handlers {
    each: RefCell<Vec<(String, Function)>>,
    /// Whether the script's first run is over: none registered then.
    closed: Cell<bool>,
}

impl Handlers {
    /// `plays.on`: run `handler` for each event of type `kind`.
    pub fn on(&self, (kind, handler): (String, Function)) -> mlua::Result<()> {
        if self.closed.get() {
            return Err(mlua::Error::runtime(
                "plays.on registers handlers as the script first runs, not after",
            ));
        }
        self.each.borrow_mut().push((kind, handler));
        Ok(())
    }

    /// Those for events of type `kind`.
    fn for_kind(&self, kind: &str) -> Vec<Function> {
        let each = self.each.borrow();
        let matching = each.iter().filter(|(of, _)| of == kind);
        matching.map(|(_, handler)| handler.clone()).collect()
    }
}

/// What a script's handlers listen with.
pub struct Listener<'a> {
    pub lua: &'a Lua,
    pub handlers: &'a Handlers,
    pub events: &'a RefCell<Events>,
    /// What calls act on: the script's window and block, or, as a handler
    /// runs, its event's.
    pub here: &'a RefCell<Target>,
    /// The window heard, with `-w`: listening ends as it closes.
    pub window: Option<&'a str>,
    pub stopped: &'a Cell<Option<u8>>,
    /// Writes a line on stderr.
    pub say: &'a dyn Fn(&str) -> io::Result<()>,
}

/// Once the script's first run is over: run its handlers for each event
/// heard, until it stops, `listening.at_most` is up, Plays quits or the
/// window heard closes. Nothing with no handlers.
pub fn listen(listener: &Listener, listening: Listening) -> Done {
    let Listener { handlers, .. } = listener;
    handlers.closed.set(true);
    let count = handlers.each.borrow().len();
    if count == 0 {
        return Ok(());
    }
    if !handlers.for_kind(POSITION).is_empty() {
        listener.events.borrow_mut().hear_positions()?;
    }
    let plural = if count == 1 { "" } else { "s" };
    (listener.say)(&format!(
        "listening: {count} handler{plural}; Ctrl-C to stop"
    ))?;
    let by = listening.at_most.map(|at_most| Instant::now() + at_most);
    loop {
        let Some((kind, payload)) = listener.events.borrow_mut().next_for_handlers(by)? else {
            return Ok(());
        };
        for handler in handlers.for_kind(&kind) {
            handle(listener, listening, &kind, &payload, &handler)?;
        }
        let window = payload.get("window_id").map(show::text);
        let closed = kind == "window.closed" && window.as_deref() == listener.window;
        if kind == "app.quit" || closed {
            return Ok(());
        }
    }
}

/// Run `handler` for the event of type `kind` with `payload`, its calls on
/// the event's window and block: a failure said, or with `fail_fast` the
/// script's.
fn handle(
    listener: &Listener,
    listening: Listening,
    kind: &str,
    payload: &Value,
    handler: &Function,
) -> Done {
    let ran = call(listener, payload, handler);
    if let Some(status) = listener.stopped.get() {
        return Err(Stop::Exit(status));
    }
    let Err(err) = ran else {
        return Ok(());
    };
    let why = format!("a {kind} handler failed: {}", message(&err));
    if listening.fail_fast {
        return Err(Stop::Script(why));
    }
    (listener.say)(&format!("plays: {why}"))?;
    Ok(())
}

/// Call `handler` with the event's `payload`, and its window and block,
/// which calls act on as it runs.
fn call(listener: &Listener, payload: &Value, handler: &Function) -> mlua::Result<()> {
    let named = |key| {
        let named = payload.get(key).filter(|name| !name.is_null());
        named.map(show::text)
    };
    let event = Target {
        window: named("window_id"),
        block: named("block_id"),
    };
    let ctx = listener.lua.create_table()?;
    ctx.set("window_id", event.window.clone())?;
    ctx.set("block_id", event.block.clone())?;
    let payload = to_lua(listener.lua, payload)?;
    let script = event.window.is_some().then(|| listener.here.replace(event));
    let ran = handler.call((ctx, payload));
    if let Some(script) = script {
        listener.here.replace(script);
    }
    ran
}

#[cfg(test)]
#[path = "tests/handlers.rs"]
mod tests;
