use super::*;

fn block(state: Value) -> Watched {
    Watched {
        window: json!("aaaa1111"),
        block: json!("bbbb2222"),
        state,
    }
}

#[test]
fn a_block_that_has_ended_has_nothing_to_wait_for() {
    let ended = block(json!({
        "label": null,
        "title": "clip.mp4",
        "window_title": "Kitchen",
        "ended": true,
        "position": 12.5,
        "duration": 12.5,
        "frame_rate": 24.0,
    }));
    assert_eq!(
        ended.already("playback.ended"),
        Some(json!({
            "window_id": "aaaa1111",
            "window_title": "Kitchen",
            "block_id": "bbbb2222",
            "block_title": "clip.mp4",
            "position": 1.0,
            "time": 12.5,
            "frame": 300,
        })),
        "as its ended event said"
    );
    let mut labeled = ended.state.clone();
    labeled["label"] = json!("left");
    assert_eq!(
        block(labeled).already("playback.ended").unwrap()["block_title"],
        "left",
        "its label, as its header shows"
    );
    assert_eq!(ended.already("playback.paused"), None, "only its ending");
    let playing = block(json!({"ended": false, "position": 3.0}));
    assert_eq!(playing.already("playback.ended"), None);
}

#[test]
fn a_block_is_heard_by_its_id_wherever_it_goes_and_a_window_by_its() {
    let watched = block(Value::Null);
    assert!(watched.hears(&json!({"window_id": "cccc3333", "block_id": "bbbb2222"})));
    assert!(!watched.hears(&json!({"window_id": "aaaa1111", "block_id": "dddd4444"})));
    assert!(
        !watched.hears(&json!({"window_id": "aaaa1111"})),
        "not its window's"
    );
    let window = Watched {
        block: Value::Null,
        ..block(Value::Null)
    };
    assert!(window.hears(&json!({"window_id": "aaaa1111", "block_id": "dddd4444"})));
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
    let watched = block(Value::Null);
    let closed = |id: &str| json!({"window_id": "aaaa1111", "block_id": id});
    assert_eq!(
        watched.gone("block.closed", &closed("bbbb2222")),
        Some("block")
    );
    assert_eq!(
        watched.gone("block.closed", &closed("dddd4444")),
        None,
        "another"
    );
    assert_eq!(watched.gone("block_added", &closed("bbbb2222")), None);
    // A window closing closes its blocks first, each heard.
    let window = Watched {
        block: Value::Null,
        ..block(Value::Null)
    };
    assert_eq!(
        window.gone("window.closed", &json!({"window_id": "aaaa1111"})),
        Some("window")
    );
    let anything = Watched {
        window: Value::Null,
        block: Value::Null,
        state: Value::Null,
    };
    assert_eq!(
        anything.gone("window.closed", &json!({"window_id": "aaaa1111"})),
        None
    );
    assert_eq!(anything.gone("block.closed", &closed("bbbb2222")), None);
}
