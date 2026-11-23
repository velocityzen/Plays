//! `plays.sleep` and `plays.wait`: a script waiting a while, or for an
//! event, as its inbox (`inbox`) hears them.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use mlua::{Function, Lua, LuaSerdeExt, Value as LuaValue};
use serde_json::{Map, Value};

use super::inbox::Inbox;

/// What a wait comes back with once its time is up.
const TIMEOUT: &str = "timeout";

/// The event told only to those who ask for positions.
pub const POSITION: &str = "playback.position";

/// `plays.sleep`: wait `seconds`.
pub fn sleep(seconds: f64) -> mlua::Result<()> {
    std::thread::sleep(duration("plays.sleep can't sleep", seconds)?);
    Ok(())
}

/// `plays.wait`: the next event of type `kind` that `filter`, if given,
/// matches, waiting at most `timeout` seconds, if given. Its payload, or
/// nil and why not.
pub fn wait(
    lua: &Lua,
    inbox: &RefCell<Inbox>,
    (kind, filter, timeout): (String, LuaValue, Option<f64>),
) -> mlua::Result<(LuaValue, Option<String>)> {
    let by = match timeout {
        Some(seconds) => Some(Instant::now() + duration("plays.wait can't wait", seconds)?),
        None => None,
    };
    let filter = Filter::read(lua, filter)?;
    if kind == POSITION
        && let Err(failure) = inbox.borrow_mut().hear_positions()
    {
        return Ok((LuaValue::Nil, Some(failure.to_string())));
    }
    inbox.borrow_mut().pass_over_told();
    heard(lua, inbox, &kind, &filter, by)
}

/// The next event of type `kind` that `filter` matches, heard by `by`.
fn heard(
    lua: &Lua,
    inbox: &RefCell<Inbox>,
    kind: &str,
    filter: &Filter,
    by: Option<Instant>,
) -> mlua::Result<(LuaValue, Option<String>)> {
    loop {
        // Not borrowed as the filter runs, which may call Plays.
        let next = inbox.borrow_mut().next(by);
        let payload = match next {
            Ok(Some((heard, payload))) if heard == kind => payload,
            Ok(Some(_)) => continue,
            Ok(None) => return Ok((LuaValue::Nil, Some(TIMEOUT.into()))),
            Err(failure) => return Ok((LuaValue::Nil, Some(failure.to_string()))),
        };
        let event = super::to_lua(lua, &payload)?;
        if filter.matches(&payload, &event)? {
            return Ok((event, None));
        }
    }
}

/// Which events a wait is for.
enum Filter {
    Any,
    /// Those whose fields equal these.
    Fields(Map<String, Value>),
    /// Those it returns true for.
    Function(Function),
}

impl Filter {
    /// `filter`, as given: nil, a table or a function.
    fn read(lua: &Lua, filter: LuaValue) -> mlua::Result<Self> {
        let wrong = || mlua::Error::runtime("plays.wait's filter is a table or a function");
        match filter {
            LuaValue::Nil => Ok(Self::Any),
            LuaValue::Function(function) => Ok(Self::Function(function)),
            LuaValue::Table(_) => match lua.from_value(filter)? {
                Value::Object(fields) => Ok(Self::Fields(fields)),
                _ => Err(wrong()),
            },
            _ => Err(wrong()),
        }
    }

    /// Whether the event with `payload`, `event` in Lua, is one of those.
    fn matches(&self, payload: &Value, event: &LuaValue) -> mlua::Result<bool> {
        match self {
            Self::Any => Ok(true),
            Self::Fields(fields) => Ok(fields
                .iter()
                .all(|(key, value)| payload.get(key).is_some_and(|field| same(field, value)))),
            Self::Function(matches) => {
                let said: LuaValue = matches.call(event.clone())?;
                Ok(!matches!(said, LuaValue::Nil | LuaValue::Boolean(false)))
            }
        }
    }
}

/// Whether `a` and `b` are the same: numbers by their values, 1 as 1.0.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => a == b,
    }
}

/// `seconds` as a duration, or why not, as `what` it's for says.
fn duration(what: &str, seconds: f64) -> mlua::Result<Duration> {
    Duration::try_from_secs_f64(seconds)
        .map_err(|_| mlua::Error::runtime(format!("{what} {seconds} seconds")))
}

#[cfg(test)]
#[path = "tests/waiting.rs"]
mod tests;
