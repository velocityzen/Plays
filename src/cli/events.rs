//! `plays events` and `plays wait`: what changes in Plays, printed as it
//! does, and waiting for one change, such as a tile ending.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Done, Session, Stop, show};
use crate::api::events::playhead;
use crate::api::socket::Heard;
use crate::api::{Error, Target};

/// Print the events heard, everything's or the window's `-w` names, until
/// `count` have been, if given, or Plays goes.
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
    }
    Ok(())
}

/// Wait for an event of type `kind` from the tile `-t` names, or else the
/// window `-w` names, or from anything, and print it; at most `timeout`
/// seconds, if given. A tile that has ended already has, and one closing
/// first fails the wait.
pub fn wait(session: &mut Session, kind: &str, timeout: Option<f64>) -> Done {
    let by = match timeout {
        Some(seconds) => match Duration::try_from_secs_f64(seconds) {
            Ok(timeout) => Some(Instant::now() + timeout),
            Err(_) => return Err(Stop::Usage(format!("can't wait {seconds} seconds"))),
        },
        None => None,
    };
    // Hearing first, so nothing happens unheard while the tile is looked at.
    session
        .client()?
        .subscribe(Heard::All, kind == "position")?;
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
        if let Some(what) = watched.gone(&heard, &payload) {
            return Err(Error::failed(format!("the {what} closed before its {kind}")).into());
        }
    }
}

/// What a wait watches: a tile, a window, or anything.
struct Watched {
    window: Value,
    tile: Value,
    /// The tile as inspected, for whether it has ended.
    state: Value,
}

/// What `-t` or `-w` names, by id; anything, with neither.
fn watched(session: &mut Session) -> Result<Watched, Stop> {
    let target = session.target();
    if target.tile.is_some() {
        let state = session.call_on("tile.inspect", Value::Null, target)?;
        return Ok(Watched {
            window: state["window"].clone(),
            tile: state["id"].clone(),
            state,
        });
    }
    if target.window.is_some() {
        let window = session.call_on(
            "window.inspect",
            Value::Null,
            Target {
                tile: None,
                ..target
            },
        )?;
        return Ok(Watched {
            window: window["id"].clone(),
            tile: Value::Null,
            state: Value::Null,
        });
    }
    Ok(Watched {
        window: Value::Null,
        tile: Value::Null,
        state: Value::Null,
    })
}

impl Watched {
    /// The event of type `kind` the tile watched had before the wait
    /// began, as it would have been heard: only its ending, as it ended,
    /// the tile titled by its label, or else what it plays.
    fn already(&self, kind: &str) -> Option<Value> {
        let state = &self.state;
        (kind == "ended" && state["ended"] == true).then(|| {
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
                "tile_id": self.tile,
                "tile_title": title,
                "position": at["position"],
                "time": at["time"],
                "frame": at["frame"],
            })
        })
    }

    /// Whether an event with `payload` is about what's watched: a tile's
    /// by its id, in whichever window it moves to.
    fn hears(&self, payload: &Value) -> bool {
        if !self.tile.is_null() {
            return payload["tile_id"] == self.tile;
        }
        self.window.is_null() || payload["window_id"] == self.window
    }

    /// What closed, if an event of type `kind`, with `payload`, says what's
    /// watched has.
    fn gone(&self, kind: &str, payload: &Value) -> Option<&'static str> {
        match kind {
            "tile_closed" if !self.tile.is_null() && payload["tile_id"] == self.tile => {
                Some("tile")
            }
            "window_closed" if !self.window.is_null() => Some("window"),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "tests/events.rs"]
mod tests;
