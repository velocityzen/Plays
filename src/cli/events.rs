//! `plays events` and `plays wait`: what changes in Plays, printed as it
//! does, and waiting for one change, such as a block ending.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Done, Session, Stop, show};
use crate::api::events::playhead;
use crate::api::socket::Heard;
use crate::api::{Error, Target};

/// Print the events heard, everything's or the window's `-w` names, until
/// `count` have been, if given, or Plays quits: `app.quit`, the last.
pub fn events(session: &mut Session, positions: bool, count: Option<usize>) -> Done {
    let heard = session
        .options
        .window
        .clone()
        .map_or(Heard::All, Heard::Window);
    let json = session.options.json;
    session.client()?.subscribe(heard, positions)?;
    for _ in 0..count.unwrap_or(usize::MAX) {
        let (kind, payload) = session.client()?.next_event()?;
        show::event(session.out, &kind, &payload, json)?;
        if kind == "app.quit" {
            break;
        }
    }
    Ok(())
}

/// Wait for an event of type `kind` from the block `-b` names, or else the
/// window `-w` names, or from anything, and print it; at most `timeout`
/// seconds, if given. A block that has ended already has, and one closing
/// first, or Plays quitting, fails the wait.
pub fn wait(session: &mut Session, kind: &str, timeout: Option<f64>) -> Done {
    let by = match timeout {
        Some(seconds) => match Duration::try_from_secs_f64(seconds) {
            Ok(timeout) => Some(Instant::now() + timeout),
            Err(_) => return Err(Stop::Usage(format!("can't wait {seconds} seconds"))),
        },
        None => None,
    };
    // Hearing first, so nothing happens unheard while the block is looked at.
    session
        .client()?
        .subscribe(Heard::All, kind == "playback.position")?;
    let watched = watched(session)?;
    if let Some(payload) = watched.already(kind) {
        return Ok(show::event(
            session.out,
            kind,
            &payload,
            session.options.json,
        )?);
    }
    loop {
        let Some((heard, payload)) = session.client()?.next_event_by(by)? else {
            let waited = timeout.unwrap_or_default();
            return Err(Error::failed(format!("no {kind} in {waited} seconds")).into());
        };
        if !watched.hears(&payload) {
            continue;
        }
        if heard == kind {
            return Ok(show::event(
                session.out,
                &heard,
                &payload,
                session.options.json,
            )?);
        }
        // What's watched, if anything, was told closing first.
        if heard == "app.quit" {
            return Err(Error::failed(format!("Plays quit before its {kind}")).into());
        }
        if let Some(what) = watched.gone(&heard, &payload) {
            return Err(Error::failed(format!("the {what} closed before its {kind}")).into());
        }
    }
}

/// What a wait watches: a block, a window, or anything.
struct Watched {
    window: Value,
    block: Value,
    /// The block as inspected, for whether it has ended.
    state: Value,
}

/// What `-b` or `-w` names, by id; anything, with neither.
fn watched(session: &mut Session) -> Result<Watched, Stop> {
    let target = session.target();
    if target.block.is_some() {
        let state = session.call_on("block.inspect", Value::Null, target)?;
        return Ok(Watched {
            window: state["window"].clone(),
            block: state["id"].clone(),
            state,
        });
    }
    if target.window.is_some() {
        let window = session.call_on(
            "window.inspect",
            Value::Null,
            Target {
                block: None,
                ..target
            },
        )?;
        return Ok(Watched {
            window: window["id"].clone(),
            block: Value::Null,
            state: Value::Null,
        });
    }
    Ok(Watched {
        window: Value::Null,
        block: Value::Null,
        state: Value::Null,
    })
}

impl Watched {
    /// The event of type `kind` the block watched had before the wait
    /// began, as it would have been heard: only its ending, as it ended,
    /// the block titled by its label, or else what it plays.
    fn already(&self, kind: &str) -> Option<Value> {
        let state = &self.state;
        (kind == "playback.ended" && state["ended"] == true).then(|| {
            let at = playhead(
                state["position"].as_f64().unwrap_or_default(),
                state["duration"].as_f64(),
                state["frame_rate"].as_f64(),
            );
            let title = match &state["label"] {
                Value::Null => &state["title"],
                label => label,
            };
            json!({
                "window_id": self.window,
                "window_title": state["window_title"],
                "block_id": self.block,
                "block_title": title,
                "position": at["position"],
                "time": at["time"],
                "frame": at["frame"],
            })
        })
    }

    /// Whether an event with `payload` is about what's watched: a block's
    /// by its id, in whichever window it moves to.
    fn hears(&self, payload: &Value) -> bool {
        if !self.block.is_null() {
            return payload["block_id"] == self.block;
        }
        self.window.is_null() || payload["window_id"] == self.window
    }

    /// What closed, if an event of type `kind`, with `payload`, says what's
    /// watched has.
    fn gone(&self, kind: &str, payload: &Value) -> Option<&'static str> {
        match kind {
            "block.closed" if !self.block.is_null() && payload["block_id"] == self.block => {
                Some("block")
            }
            "window.closed" if !self.window.is_null() => Some("window"),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "tests/events.rs"]
mod tests;
