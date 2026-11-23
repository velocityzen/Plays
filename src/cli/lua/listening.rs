//! What a script does once its first run is over, if it has handlers
//! (`handlers`) or timers (`timers`): runs each as its event comes or its
//! time is up, one at a time, until it stops, `--for` is up, nothing's
//! left to run, Plays quits, or the window it hears, with `-w`, closes;
//! Ctrl-C, which nothing catches, stops it too.

use std::cell::{Cell, RefCell};
use std::io;
use std::time::{Duration, Instant};

use mlua::Lua;

use super::handlers::Handlers;
use super::message;
use super::timers::Timers;
use super::waiting::{Events, POSITION};
use crate::api::Target;
use crate::cli::{Done, Stop, show};

/// The least a script waits for events, though a timer is overdue: so it
/// hears those come meanwhile.
const HEAR_AT_LEAST: Duration = Duration::from_millis(1);

/// How long a script with handlers or timers runs on, and whether one
/// failing ends it.
#[derive(Clone, Copy, Debug)]
pub struct Listening {
    pub at_most: Option<Duration>,
    pub fail_fast: bool,
}

/// What a script's handlers and timers run with.
pub struct Listener<'a> {
    pub lua: &'a Lua,
    pub handlers: &'a Handlers,
    pub timers: &'a Timers,
    pub events: &'a RefCell<Events>,
    /// What calls act on: the script's window and block, or, as a handler
    /// runs, its event's.
    pub here: &'a RefCell<Target>,
    /// The window heard, with `-w`: listening ends as it closes.
    pub window: Option<&'a str>,
    pub stopped: &'a Cell<Option<u8>>,
    /// Writes a line on stderr.
    pub say: &'a dyn Fn(&str) -> io::Result<()>,
    pub listening: Listening,
}

/// Once the script's first run is over: run its handlers for each event
/// heard and its timers as they're due, until it stops, `--for` is up,
/// none is left to run, Plays quits or the window heard closes. Nothing
/// with neither.
pub fn listen(listener: &Listener) -> Done {
    let handlers = listener.handlers.close();
    let timers = listener.timers.count();
    if handlers == 0 && timers == 0 {
        return Ok(());
    }
    if listener.handlers.hear(POSITION) {
        listener.events.borrow_mut().hear_positions()?;
    }
    (listener.say)(&format!("{}; Ctrl-C to stop", going(handlers, timers)))?;
    let by = listener
        .listening
        .at_most
        .map(|at_most| Instant::now() + at_most);
    loop {
        // Those due by now, each once: one taking longer than its period
        // doesn't keep the rest waiting.
        let now = Instant::now();
        while let Some(timer) = listener.timers.take_due(now) {
            listener.ran("a timer", timer.call(()))?;
        }
        let due = listener.timers.next_due();
        if by.is_some_and(|by| by <= Instant::now()) || (handlers == 0 && due.is_none()) {
            return Ok(());
        }
        let wake = [by, due].into_iter().flatten().min();
        if handlers == 0 {
            let wake = wake.expect("a timer due");
            std::thread::sleep(wake.saturating_duration_since(Instant::now()));
            continue;
        }
        let wake = wake.map(|wake| wake.max(Instant::now() + HEAR_AT_LEAST));
        let Some((kind, payload)) = listener.events.borrow_mut().next_for_handlers(wake)? else {
            continue;
        };
        for handler in listener.handlers.for_kind(&kind) {
            let ran = super::handlers::call(listener, &payload, &handler);
            listener.ran(&format!("a {kind} handler"), ran)?;
        }
        let window = payload.get("window_id").map(show::text);
        let closed = kind == "window.closed" && window.as_deref() == listener.window;
        if kind == "app.quit" || closed {
            return Ok(());
        }
    }
}

/// What a script runs as it listens: `handlers` and `timers`, by how many.
fn going(handlers: usize, timers: usize) -> String {
    let some = |count: usize, what: &str| {
        let plural = if count == 1 { "" } else { "s" };
        format!("{count} {what}{plural}")
    };
    match (handlers, timers) {
        (_, 0) => format!("listening: {}", some(handlers, "handler")),
        (0, _) => format!("running: {}", some(timers, "timer")),
        _ => format!(
            "listening: {}, running: {}",
            some(handlers, "handler"),
            some(timers, "timer")
        ),
    }
}

impl Listener<'_> {
    /// What `what`, a handler or a timer, running came to: its failure
    /// said, or with `--fail-fast` the script's; the script stopped, if it
    /// did.
    fn ran(&self, what: &str, ran: mlua::Result<()>) -> Done {
        if let Some(status) = self.stopped.get() {
            return Err(Stop::Exit(status));
        }
        let Err(err) = ran else {
            return Ok(());
        };
        let why = format!("{what} failed: {}", message(&err));
        if self.listening.fail_fast {
            return Err(Stop::Script(why));
        }
        (self.say)(&format!("plays: {why}"))?;
        Ok(())
    }
}
