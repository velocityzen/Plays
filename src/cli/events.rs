//! `plays events` and `plays wait`: what changes in Plays, printed as it
//! does, and waiting for one change, such as a tile ending.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Done, Session, Stop, show};
use crate::api::socket::Heard;
use crate::api::{Error, Target};

/// Print the events heard, the app's, a window's or all, until `count`
/// have been, if given, or Plays goes.
pub fn events(session: &mut Session, all: bool, positions: bool, count: Option<usize>) -> Done {
    let heard = match (all, session.options.window.clone()) {
        (true, Some(_)) => return Err(Stop::Usage("hear everything, or -w's window".into())),
        (true, None) => Heard::All,
        (false, Some(window)) => Heard::Window(window),
        (false, None) => Heard::App,
    };
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
    /// began, as it would have been heard: only its ending, as it ended.
    fn already(&self, kind: &str) -> Option<Value> {
        (kind == "ended" && self.state["ended"] == true).then(|| {
            json!({
                "window_id": self.window,
                "tile_id": self.tile,
                "position": self.state["position"],
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
mod tests {
    use super::*;

    fn tile(state: Value) -> Watched {
        Watched {
            window: json!("aaaa1111"),
            tile: json!("bbbb2222"),
            state,
        }
    }

    #[test]
    fn a_tile_that_has_ended_has_nothing_to_wait_for() {
        let ended = tile(json!({"ended": true, "position": 12.5}));
        assert_eq!(
            ended.already("ended"),
            Some(json!({"window_id": "aaaa1111", "tile_id": "bbbb2222", "position": 12.5}))
        );
        assert_eq!(ended.already("paused"), None, "only its ending");
        let playing = tile(json!({"ended": false, "position": 3.0}));
        assert_eq!(playing.already("ended"), None);
    }

    #[test]
    fn a_tile_is_heard_by_its_id_wherever_it_goes_and_a_window_by_its() {
        let watched = tile(Value::Null);
        assert!(watched.hears(&json!({"window_id": "cccc3333", "tile_id": "bbbb2222"})));
        assert!(!watched.hears(&json!({"window_id": "aaaa1111", "tile_id": "dddd4444"})));
        assert!(
            !watched.hears(&json!({"window_id": "aaaa1111"})),
            "not its window's"
        );
        let window = Watched {
            tile: Value::Null,
            ..tile(Value::Null)
        };
        assert!(window.hears(&json!({"window_id": "aaaa1111", "tile_id": "dddd4444"})));
        assert!(!window.hears(&json!({"window_id": "cccc3333"})));
        let anything = Watched {
            window: Value::Null,
            ..window
        };
        assert!(anything.hears(&json!({"window_id": "cccc3333"})));
        assert!(anything.hears(&json!({})));
    }

    #[test]
    fn closing_what_is_watched_ends_the_wait() {
        let watched = tile(Value::Null);
        let closed = |id: &str| json!({"window_id": "aaaa1111", "tile_id": id});
        assert_eq!(
            watched.gone("tile_closed", &closed("bbbb2222")),
            Some("tile")
        );
        assert_eq!(
            watched.gone("tile_closed", &closed("dddd4444")),
            None,
            "another"
        );
        assert_eq!(watched.gone("tile_added", &closed("bbbb2222")), None);
        // A window closing closes its tiles first, each heard.
        let window = Watched {
            tile: Value::Null,
            ..tile(Value::Null)
        };
        assert_eq!(
            window.gone("window_closed", &json!({"window_id": "aaaa1111"})),
            Some("window")
        );
        let anything = Watched {
            window: Value::Null,
            tile: Value::Null,
            state: Value::Null,
        };
        assert_eq!(
            anything.gone("window_closed", &json!({"window_id": "aaaa1111"})),
            None
        );
        assert_eq!(anything.gone("tile_closed", &closed("bbbb2222")), None);
    }
}
