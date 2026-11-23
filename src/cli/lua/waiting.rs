//! `plays.sleep` and `plays.wait`: a script waiting a while, or for an
//! event. Events come on a connection of their own, subscribed at the
//! script's first call or wait, so calls and events don't wait on each
//! other. A wait hears what happens from the start of the last call
//! before it, or from its own start if there's none: each event once, in
//! order, those it passes over gone.

use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlua::{Function, Lua, LuaSerdeExt, Value as LuaValue};
use serde_json::{Map, Value};

use crate::api::socket::Heard;
use crate::api::socket::client::{Client, Failure};

/// What a wait comes back with once its time is up.
const TIMEOUT: &str = "timeout";

/// The event told only to those who ask for positions.
const POSITION: &str = "playback.position";

/// The events a script hears.
pub struct Events {
    socket: Option<PathBuf>,
    client: Option<Client>,
}

impl Events {
    /// Events from the Plays answering at `socket`, not heard yet.
    pub fn new(socket: Option<PathBuf>) -> Self {
        Self {
            socket,
            client: None,
        }
    }

    /// As a call begins: hear what happens from now on, and nothing told
    /// before. Failing, the call goes on: a wait after says why.
    pub fn hear_from_now(&mut self) {
        let heard = match &mut self.client {
            Some(client) => client.forget_events(),
            None => self.client().map(drop),
        };
        if let Err(failure) = heard {
            log::debug!("can't hear events: {failure}");
        }
    }

    /// The connection events come on, every one heard.
    fn client(&mut self) -> Result<&mut Client, Failure> {
        if self.client.is_none() {
            let Some(path) = self.socket.as_deref() else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "no socket").into());
            };
            let mut client = Client::connect(path)?;
            client.subscribe(Heard::All, false)?;
            self.client = Some(client);
        }
        Ok(self.client.as_mut().expect("connected"))
    }

    /// Hear where files are as they play too, or not.
    fn hear_positions(&mut self, positions: bool) -> Result<(), Failure> {
        self.client()?.subscribe(Heard::All, positions).map(drop)
    }

    /// The next event heard, by its type, with its payload, waiting for it
    /// until `by`, if given: none by then.
    fn next(&mut self, by: Option<Instant>) -> Result<Option<(String, Value)>, Failure> {
        self.client()?.next_event_by(by)
    }
}

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
    events: &RefCell<Events>,
    (kind, filter, timeout): (String, LuaValue, Option<f64>),
) -> mlua::Result<(LuaValue, Option<String>)> {
    let by = match timeout {
        Some(seconds) => Some(Instant::now() + duration("plays.wait can't wait", seconds)?),
        None => None,
    };
    let filter = Filter::read(lua, filter)?;
    let positions = kind == POSITION;
    if positions && let Err(failure) = events.borrow_mut().hear_positions(true) {
        return Ok((LuaValue::Nil, Some(failure.to_string())));
    }
    let heard = heard(lua, events, &kind, &filter, by);
    if positions {
        events.borrow_mut().hear_positions(false).ok();
    }
    heard
}

/// The next event of type `kind` that `filter` matches, heard by `by`.
fn heard(
    lua: &Lua,
    events: &RefCell<Events>,
    kind: &str,
    filter: &Filter,
    by: Option<Instant>,
) -> mlua::Result<(LuaValue, Option<String>)> {
    loop {
        // Not borrowed as the filter runs, which may call Plays.
        let next = events.borrow_mut().next(by);
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
