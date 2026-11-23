//! `plays.on`: handlers a script registers as it first runs, for events by
//! their type, each run as one comes once that run is over (`listening`).
//! One runs at a time, so a wait or a sleep in one holds the others, and a
//! handler's calls act on its event's window and block.

use std::cell::{Cell, RefCell};

use mlua::Function;
use serde_json::Value;

use super::listening::Listener;
use super::to_lua;
use crate::api::Target;
use crate::cli::show;

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

    /// As the script's first run ends: none registered from now on. How
    /// many there are.
    pub fn close(&self) -> usize {
        self.closed.set(true);
        self.each.borrow().len()
    }

    /// Whether any is for events of type `kind`.
    pub fn hear(&self, kind: &str) -> bool {
        self.each.borrow().iter().any(|(of, _)| of == kind)
    }

    /// Those for events of type `kind`.
    pub fn for_kind(&self, kind: &str) -> Vec<Function> {
        let each = self.each.borrow();
        let matching = each.iter().filter(|(of, _)| of == kind);
        matching.map(|(_, handler)| handler.clone()).collect()
    }
}

/// Call `handler` with the event's `payload`, and its window and block,
/// which calls act on as it runs.
pub fn call(listener: &Listener, payload: &Value, handler: &Function) -> mlua::Result<()> {
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
