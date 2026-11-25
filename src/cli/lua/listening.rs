//! What a script does once its first run is over, if it has handlers
//! (`handlers`) or timers (`timers`): runs each as its event comes or its
//! time is up, and hands its detached programs' output to their functions
//! (`exec`), one at a time, each a part of the script of its own, as its
//! inbox (`inbox`) has them; until it stops, `--for` is up, no handler or
//! timer is left, Plays quits, or the window it hears, with `-w`, closes.
//! Handlers for cues are left until theirs are reached.
//! Ctrl-C stops it too (`stopping`).

use std::cell::{Cell, RefCell};
use std::io;
use std::time::{Duration, Instant};

use mlua::Lua;
use serde_json::Value;

use super::exec::{Programs, Ran};
use super::handlers::Handlers;
use super::inbox::{Inbox, Next};
use super::message;
use super::stopping;
use super::timers::Timers;
use super::waiting::POSITION;
use crate::api::Target;
use crate::api::events::cues::is_cue;
use crate::cli::{Done, Stop, after, show};

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
    pub programs: &'a Programs,
    pub inbox: &'a RefCell<Inbox>,
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
/// heard, its timers as they're due and its programs' functions as their
/// output comes, until it stops, `--for` is up, no handler or timer is
/// left, Plays quits or the window heard closes. Nothing with neither.
pub fn listen(listener: &Listener) -> Done {
    let handlers = listener.handlers.close();
    let timers = listener.timers.count();
    if handlers == 0 && timers == 0 {
        return Ok(());
    }
    if listener.handlers.hear(POSITION) {
        listener.inbox.borrow_mut().hear_positions()?;
    }
    for cue in listener.handlers.cues() {
        let block = listener.inbox.borrow_mut().cue(&cue)?;
        listener.handlers.cued(&cue, block);
    }
    (listener.say)(&format!("{}; Ctrl-C to stop", going(handlers, timers)))?;
    let by = listener.listening.at_most.map(after);
    loop {
        if stopping::caught() {
            return Err(Stop::Exit(stopping::INTERRUPTED_STATUS));
        }
        // Those due by now, each once: one taking longer than its period
        // doesn't keep the rest waiting.
        let now = Instant::now();
        while let Some(timer) = listener.timers.take_due(now) {
            let ran = listener.programs.part(|| timer.call(()));
            listener.ran("a timer", ran)?;
        }
        let due = listener.timers.next_due();
        let handlers = listener.handlers.count();
        if by.is_some_and(|by| by <= Instant::now()) || (handlers == 0 && due.is_none()) {
            return Ok(());
        }
        let wake = [by, due].into_iter().flatten().min();
        let next = listener
            .inbox
            .borrow_mut()
            .next_listening(wake, handlers > 0)?;
        match next {
            Some(Next::Event(kind, payload)) => {
                if listener.hear(&kind, &payload)? {
                    return Ok(());
                }
            }
            Some(Next::Ran(ran)) => listener.take(ran)?,
            None => {}
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
    /// Run the handlers for the event of type `kind` with `payload`, those
    /// for a cue once: whether listening's over, Plays quitting or the
    /// window heard closing. A block closing takes the cues on it, unrun.
    fn hear(&self, kind: &str, payload: &Value) -> Result<bool, Stop> {
        let handlers = self.handlers.for_kind(kind);
        if is_cue(kind) {
            self.handlers.spend(kind);
        }
        for handler in handlers {
            let ran = self
                .programs
                .part(|| super::handlers::call(self, payload, &handler));
            self.ran(&format!("a {kind} handler"), ran)?;
        }
        if kind == "block.closed" {
            let block = payload.get("block_id").map(show::text).unwrap_or_default();
            for cue in self.handlers.block_closed(&block) {
                (self.say)(&format!("plays: {cue}: the block closed before it"))?;
            }
        }
        let window = payload.get("window_id").map(show::text);
        let closed = kind == "window.closed" && window.as_deref() == self.window;
        Ok(kind == "app.quit" || closed)
    }

    /// Take in what a program did: a piece of its output handed to its
    /// function, if it has one.
    fn take(&self, ran: Ran) -> Done {
        let Some(call) = self.programs.take(ran) else {
            return Ok(());
        };
        let what = call.what().to_owned();
        let ran = self.programs.part(|| call.run(self.lua));
        self.ran(&what, ran)
    }

    /// What `what`, a handler, a timer or a function, running came to: its
    /// failure said, or with `--fail-fast` the script's; the script
    /// stopped, if it did.
    fn ran(&self, what: &str, ran: mlua::Result<()>) -> Done {
        if let Some(status) = self.stopped.get() {
            return Err(Stop::Exit(status));
        }
        if stopping::caught() {
            return Err(Stop::Exit(stopping::INTERRUPTED_STATUS));
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
