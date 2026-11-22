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
