//! What goes over the socket: JSON, an object to a line, each way.
//!
//! A client asks, each request with an `id` of its choosing, which its
//! answer carries back:
//!
//! - `{"id": 1, "method": "call", "command": "playback.seek", "args":
//!   {"seconds": 30}, "target": {"window": "2"}}` runs a command, as a key
//!   would; `args` and `target` may be left out.
//! - `{"method": "commands"}` lists every command, as `describe` describes
//!   one: `{"method": "describe", "command": "playback.seek"}`.
//! - `{"method": "subscribe", "scope": "all", "positions": true}` hears
//!   events from now on: the app's (`"app"`), one window's (`{"window":
//!   "2"}`) or, as it is left out, all; with positions, at most once a
//!   second. `{"method": "unsubscribe"}` hears none.
//! - `{"method": "hello"}` asks which Plays answers: its version and
//!   process id.
//! - `{"method": "hand_over", "urls": [...]}` is what a Plays started with
//!   files hands the one running: it opens them and comes to the front.
//!
//! Plays answers each in turn: `{"id": 1, "result": ...}`, or why not,
//! as `api::Error` says it, always with a message: `{"id": 1, "error":
//! "not_found", "what": "window", "name": "9", "message": "no window 9"}`;
//! a request it can't read is a `bad_request`. Events come between answers
//! as they happen, each `{"type": ..., "payload": ...}`, with no id: those
//! told as a request runs, such as its command's `app.command_run`, before its
//! answer, and what the command changes after, once that has settled.
//! Readers ignore fields they don't know, so later versions can add them.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{Error, Target};

/// A request, and the id its answer carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub id: Value,
    #[serde(flatten)]
    pub ask: Ask,
}

/// What a client asks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Ask {
    /// Which Plays this is.
    Hello,
    /// Run `command`.
    Call {
        command: String,
        #[serde(default, skip_serializing_if = "Value::is_null")]
        args: Value,
        #[serde(default, skip_serializing_if = "Target::is_empty")]
        target: Target,
    },
    /// Every command, described.
    Commands,
    /// `command`, described.
    Describe { command: String },
    /// Hear events from now on, and only these, with positions or not.
    Subscribe {
        #[serde(default)]
        scope: Heard,
        #[serde(default)]
        positions: bool,
    },
    /// Hear none.
    Unsubscribe,
    /// Open `urls`, files by their full paths and streams, and come to the
    /// front; with none, bring the window in front forward.
    HandOver {
        #[serde(default)]
        urls: Vec<String>,
    },
}

/// Whose events a client hears: a window by name, as a target names it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Heard {
    App,
    #[default]
    All,
    Window(String),
}

/// What a request comes back with, by its `id`.
pub fn answer(id: &Value, outcome: Result<Value, Error>) -> Value {
    match outcome {
        Ok(result) => json!({"id": id, "result": result}),
        Err(err) => {
            let mut answer = serde_json::Map::new();
            answer.insert("id".into(), id.clone());
            if let Ok(Value::Object(why)) = serde_json::to_value(&err) {
                answer.extend(why);
            }
            // Bad arguments and failures say why in their own.
            answer
                .entry("message")
                .or_insert_with(|| err.to_string().into());
            Value::Object(answer)
        }
    }
}

/// What a request Plays can't read comes back with: `why`.
pub fn bad_request(id: &Value, why: &str) -> Value {
    json!({"id": id, "error": "bad_request", "message": why})
}

/// Read `line` as a request, or say why it isn't one, with its id if it
/// has one.
pub fn read(line: &str) -> Result<Request, (Value, String)> {
    let value: Value = serde_json::from_str(line).map_err(|err| (Value::Null, err.to_string()))?;
    let id = value.get("id").cloned().unwrap_or_default();
    serde_json::from_value(value).map_err(|err| (id, err.to_string()))
}

/// What came over the socket, read by a client.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// An event: its type and payload.
    Event { kind: String, payload: Value },
    /// The answer to the request with `id`.
    Answer {
        id: Value,
        outcome: Result<Value, Refusal>,
    },
}

/// Why a request didn't do what it asked.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// The command didn't run, or couldn't do it.
    Command(Error),
    /// Plays couldn't read the request, or this couldn't read the answer.
    BadRequest(String),
}

impl Incoming {
    /// Read `line`, as Plays sends it.
    pub fn read(line: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
        if let Some(kind) = value.get("type").and_then(Value::as_str) {
            return Ok(Self::Event {
                kind: kind.to_owned(),
                payload: value.get("payload").cloned().unwrap_or_default(),
            });
        }
        let id = value.get("id").cloned().unwrap_or_default();
        let outcome = if let Some(result) = value.get("result") {
            Ok(result.clone())
        } else if value.get("error").is_some() {
            let message = value.get("message").and_then(Value::as_str);
            let message = message.unwrap_or("Plays didn't say why").to_owned();
            // A bad request, or an error a later Plays has, is told as said.
            Err(serde_json::from_value(value)
                .map_or(Refusal::BadRequest(message), Refusal::Command))
        } else {
            return Err(format!("neither an answer nor an event: {line}"));
        };
        Ok(Self::Answer { id, outcome })
    }
}

#[cfg(test)]
#[path = "tests/protocol.rs"]
mod tests;
