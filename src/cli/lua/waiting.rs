//! `plays.sleep` and `plays.wait`: a script waiting a while, or for an
//! event, as its inbox (`inbox`) hears them.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use mlua::{Function, IntoLuaMulti, Lua, LuaSerdeExt, MultiValue, Scope, Table, Value as LuaValue};
use serde_json::{Map, Value};

use super::Answer;
use super::inbox::Inbox;
use super::stopping::{self, unless_caught};
use crate::api::events::cues::is_cue;
use crate::cli::after;

/// What a wait comes back with once its time is up.
const TIMEOUT: &str = "timeout";

/// The event told only to those who ask for positions.
pub const POSITION: &str = "playback.position";

/// `plays.sleep`, `plays.wait` and `plays.mark`, added to `plays`, as
/// long as `scope` is.
pub fn add<'s, 'env>(
    lua: &Lua,
    plays: &Table,
    scope: &'s Scope<'s, 'env>,
    inbox: &'env RefCell<Inbox>,
) -> mlua::Result<()> {
    // Each cut short by Ctrl-C fails, not as if it waited.
    plays.set(
        "sleep",
        lua.create_function(|_, seconds: f64| sleep(seconds))?,
    )?;
    let waits = scope.create_function(|lua, args| {
        let waited = wait(lua, inbox, args);
        unless_caught().and(waited)
    })?;
    plays.set("wait", waits)?;
    let marks = scope.create_function(|_, ()| {
        let marked = mark(inbox);
        unless_caught().map(|()| marked)
    })?;
    plays.set("mark", marks)
}

/// `plays.sleep`: wait `seconds`.
fn sleep(seconds: f64) -> mlua::Result<()> {
    stopping::sleep(duration("plays.sleep can't sleep", seconds)?);
    unless_caught()
}

/// `plays.mark`: where a wait given it hears from, now; or nil and why
/// not.
fn mark(inbox: &RefCell<Inbox>) -> Answer {
    Answer(match inbox.borrow_mut().mark() {
        Ok(mark) => Ok(LuaValue::Integer(i64::try_from(mark).unwrap_or(i64::MAX))),
        Err(failure) => Err(failure.to_string()),
    })
}

/// What a wait hands back: the event's payload, and its type if it was
/// one of a list; or nil and why not.
struct Waited {
    heard: Result<(LuaValue, String), String>,
    listed: bool,
}

impl IntoLuaMulti for Waited {
    fn into_lua_multi(self, lua: &Lua) -> mlua::Result<MultiValue> {
        match self.heard {
            Ok((event, kind)) if self.listed => (event, kind).into_lua_multi(lua),
            Ok((event, _)) => event.into_lua_multi(lua),
            Err(why) => (LuaValue::Nil, why).into_lua_multi(lua),
        }
    }
}

/// `plays.wait`: the next event of type `kinds`, or of any type in a list
/// of them, that `filter`, if given, matches, waiting at most `timeout`
/// seconds, if given, and heard from `mark`, if given; or, for a cue, its
/// block reaching it, which fails as the block closes first, with no
/// other type to wait for.
fn wait(
    lua: &Lua,
    inbox: &RefCell<Inbox>,
    (kinds, filter, timeout, mark): (LuaValue, LuaValue, Option<f64>, Option<u64>),
) -> mlua::Result<Waited> {
    let (kinds, listed) = read_kinds(lua, kinds)?;
    let by = match timeout {
        Some(seconds) => Some(after(duration("plays.wait can't wait", seconds)?)),
        None => None,
    };
    let filter = Filter::read(lua, filter)?;
    let failed = |why: String| {
        Ok(Waited {
            heard: Err(why),
            listed,
        })
    };
    let mut cued = Vec::new();
    for kind in kinds.iter().filter(|kind| is_cue(kind)) {
        match inbox.borrow_mut().cue(kind) {
            Ok(block) => cued.push((kind.clone(), block)),
            Err(failure) => return failed(failure.to_string()),
        }
    }
    if kinds.iter().any(|kind| kind == POSITION)
        && let Err(failure) = inbox.borrow_mut().hear_positions()
    {
        return failed(failure.to_string());
    }
    inbox.borrow_mut().pass_over_told(mark);
    let heard = heard(lua, inbox, (&kinds, &cued), &filter, by)?;
    Ok(Waited { heard, listed })
}

/// The types a wait is for, given as one or a list, and whether a list.
fn read_kinds(lua: &Lua, given: LuaValue) -> mlua::Result<(Vec<String>, bool)> {
    let wrong = || mlua::Error::runtime("plays.wait waits for an event's type, or a list of them");
    match given {
        LuaValue::String(kind) => Ok((vec![kind.to_str()?.to_owned()], false)),
        LuaValue::Table(_) => match lua.from_value::<Vec<String>>(given) {
            Ok(kinds) if !kinds.is_empty() => Ok((kinds, true)),
            _ => Err(wrong()),
        },
        _ => Err(wrong()),
    }
}

/// The next event of a type in `kinds` that `filter` matches, heard by
/// `by`, with its type; none once each cue in `kinds` has had its block
/// close, as `cued` says them, if they're all there is.
fn heard(
    lua: &Lua,
    inbox: &RefCell<Inbox>,
    (kinds, cued): (&[String], &[(String, String)]),
    filter: &Filter,
    by: Option<Instant>,
) -> mlua::Result<Result<(LuaValue, String), String>> {
    let mut waiting: Vec<&String> = kinds.iter().collect();
    loop {
        // Not borrowed as the filter runs, which may call Plays.
        let next = inbox.borrow_mut().next(by);
        let (kind, payload) = match next {
            Ok(Some(event)) => event,
            Ok(None) => return Ok(Err(TIMEOUT.into())),
            Err(failure) => return Ok(Err(failure.to_string())),
        };
        if kinds.contains(&kind) {
            let event = super::to_lua(lua, &payload)?;
            if filter.matches(&payload, &event)? {
                return Ok(Ok((event, kind)));
            }
        }
        if kind == "block.closed" {
            let closed = |cue: &&String| {
                cued.iter()
                    .any(|(of, block)| of == *cue && payload["block_id"] == **block)
            };
            if waiting.iter().any(closed) {
                waiting.retain(|cue| !closed(cue));
                if waiting.is_empty() {
                    return Ok(Err(format!(
                        "the block closed before {}",
                        kinds.join(" or ")
                    )));
                }
            }
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
