use serde_json::{Value, json};

use super::*;
use crate::api::events::Scope;

/// A block that hasn't opened its file yet.
fn block(id: Id) -> BlockSeen {
    BlockSeen {
        id,
        label: None,
        title: "clip.mp4".into(),
        opening: 0,
        opened: None,
        failed: None,
        playing: false,
        ended: false,
        seeks: 0,
        sought: 0.0,
        position: 0.0,
        duration: None,
        frame_rate: None,
        volume: 100.0,
        muted: false,
        tracks: [None; 3],
        chapter: None,
        casting: None,
        picture_in_picture: false,
        over_video: OverVideo::default(),
    }
}

/// Where a block is, as an event says: how far through, in seconds and by
/// frame.
fn at(position: f64, time: f64, frame: u64) -> Value {
    json!({"position": position, "time": time, "frame": frame})
}

/// `block`, its file opened.
fn opened(block: BlockSeen) -> BlockSeen {
    BlockSeen {
        opened: Some(json!({"title": "clip.mp4", "duration": 10.0})),
        duration: Some(10.0),
        frame_rate: Some(25.0),
        ..block
    }
}

/// A window with `blocks`, keys going to the first.
fn window(id: Id, blocks: Vec<BlockSeen>) -> WindowSeen {
    WindowSeen {
        id,
        label: None,
        title: "Plays".into(),
        full_screen: false,
        over_video: OverVideo::default(),
        active: blocks.first().map(|block| block.id),
        maximized: None,
        blocks,
    }
}

/// The app with `windows`, the first in front.
fn app(windows: Vec<WindowSeen>) -> Seen {
    Seen {
        front: windows.first().map(|window| window.id),
        windows,
        ..Seen::default()
    }
}

/// `fields` with `more`.
fn with(fields: &Value, more: Value) -> Value {
    let mut fields = fields.clone();
    if let (Value::Object(fields), Value::Object(more)) = (&mut fields, more) {
        fields.extend(more);
    }
    fields
}

fn kinds(events: &[Event]) -> Vec<&str> {
    events.iter().map(|event| event.kind).collect()
}

/// What's told going from one block, `old`, to `new`, in one window.
fn block_changes(old: &BlockSeen, new: &BlockSeen) -> Vec<Event> {
    let w = Id::next();
    changes(
        &app(vec![window(w, vec![old.clone()])]),
        &app(vec![window(w, vec![new.clone()])]),
    )
}

/// What `events` say, without the ids and titles.
fn said(events: &[Event]) -> Vec<(&str, Value)> {
    events
        .iter()
        .map(|event| {
            let mut payload = event.payload.clone();
            if let Value::Object(fields) = &mut payload {
                for name in ["window_id", "window_title", "block_id", "block_title"] {
                    fields.remove(name);
                }
            }
            (event.kind, payload)
        })
        .collect()
}

#[test]
fn windows_and_blocks_are_told_as_they_open_and_close_once_each() {
    let (w, t) = (Id::next(), Id::next());
    let none = Seen::default();
    let one = app(vec![window(w, vec![block(t)])]);

    let events = changes(&none, &one);
    assert_eq!(
        kinds(&events),
        [
            "window.created",
            "block.created",
            "window.layout_changed",
            "block.active_changed",
            "window.active_changed",
        ]
    );
    // Each named by its id and title.
    let (w, t) = (w.to_string(), t.to_string());
    assert_eq!(
        events[0].payload,
        json!({"window_id": w, "window_title": "Plays", "label": null})
    );
    let block_ids = json!({
        "window_id": w,
        "window_title": "Plays",
        "block_id": t,
        "block_title": "clip.mp4",
    });
    assert_eq!(events[1].payload, with(&block_ids, json!({"label": null})));
    assert_eq!(
        events[2].payload,
        json!({
            "window_id": w,
            "window_title": "Plays",
            "blocks": [t],
            "maximized": null,
        })
    );
    assert_eq!(events[3].payload, block_ids, "the block keys go to");
    assert_eq!(
        events[4].payload,
        json!({"window_id": w, "window_title": "Plays"})
    );
    assert!(changes(&one, &one).is_empty(), "nothing told twice");

    let events = changes(&one, &none);
    assert_eq!(
        kinds(&events),
        ["block.closed", "window.closed", "window.active_changed"]
    );
    assert_eq!(events[0].payload, block_ids, "as they were");
    assert_eq!(
        events[1].payload,
        json!({"window_id": w, "window_title": "Plays"})
    );
    assert_eq!(
        events[2].payload,
        json!({"window_id": null, "window_title": null})
    );
}

#[test]
fn labels_full_screen_the_layout_and_the_block_keys_go_to_are_told_as_they_change() {
    let (w, t, u) = (Id::next(), Id::next(), Id::next());
    let before = app(vec![window(w, vec![block(t), block(u)])]);
    let mut after = before.clone();
    let changed = &mut after.windows[0];
    changed.label = Some("news".into());
    changed.title = "news".into();
    changed.full_screen = true;
    changed.blocks[1].label = Some("left".into());
    changed.blocks[1].title = "left".into();
    changed.blocks.swap(0, 1);
    changed.maximized = Some(u);
    changed.active = Some(u);

    let events = changes(&before, &after);
    assert_eq!(
        said(&events),
        [
            ("window.label_changed", json!({"label": "news"})),
            ("window.full_screen_changed", json!({"full_screen": true})),
            (
                "window.layout_changed",
                json!({
                    "blocks": [u.to_string(), t.to_string()],
                    "maximized": u.to_string(),
                })
            ),
            ("block.active_changed", json!({})),
            // Then the blocks, as they now are.
            ("block.label_changed", json!({"label": "left"})),
        ]
    );
    // Each by its title as it now is.
    assert!(
        events
            .iter()
            .all(|event| event.payload["window_title"] == "news"),
        "{events:?}"
    );
    assert_eq!(
        events[3].payload["block_title"], "left",
        "the block keys go to"
    );
    assert_eq!(events[4].payload["block_title"], "left");
    assert!(changes(&after, &after).is_empty());
}

#[test]
fn the_panels_a_window_and_its_blocks_show_are_told_as_they_change() {
    let (w, t, u) = (Id::next(), Id::next(), Id::next());
    let before = app(vec![window(w, vec![block(t), block(u)])]);
    let mut after = before.clone();
    let changed = &mut after.windows[0];
    changed.over_video.notes = false;
    changed.blocks[1].over_video = OverVideo {
        top_bar: true,
        ..OverVideo::NONE
    };

    let events = changes(&before, &after);
    assert_eq!(
        said(&events),
        [
            (
                "window.ui_changed",
                json!({"visible": ["controls", "top_bar", "track_switcher"]})
            ),
            ("block.ui_changed", json!({"visible": ["top_bar"]})),
        ]
    );
    assert_eq!(events[1].payload["block_id"], u.to_string(), "its own");
    assert!(changes(&after, &after).is_empty());

    // One opened in it starts out as it is: its `block.created` says so.
    let mut more = after.clone();
    more.windows[0].blocks.push(BlockSeen {
        over_video: OverVideo::NONE,
        ..block(Id::next())
    });
    let told = changes(&after, &more);
    assert!(
        !kinds(&told).contains(&"block.ui_changed"),
        "{:?}",
        kinds(&told)
    );
}

#[test]
fn the_window_in_front_and_the_block_keys_go_to_are_named_by_their_titles() {
    let (w, x, t, u) = (Id::next(), Id::next(), Id::next(), Id::next());
    let kitchen = WindowSeen {
        title: "Kitchen".into(),
        ..window(x, Vec::new())
    };
    let before = app(vec![window(w, vec![block(t), block(u)]), kitchen]);
    let mut after = before.clone();
    after.front = Some(x);
    let changed = &mut after.windows[0];
    changed.blocks[1].label = Some("left".into());
    changed.blocks[1].title = "left".into();
    changed.active = Some(u);

    let events = changes(&before, &after);
    let told = |kind| {
        let event = events.iter().find(|event| event.kind == kind);
        event.map(|event| event.payload.clone())
    };
    assert_eq!(
        told("block.active_changed"),
        Some(json!({
            "window_id": w.to_string(),
            "window_title": "Plays",
            "block_id": u.to_string(),
            "block_title": "left",
        }))
    );
    assert_eq!(
        told("window.active_changed"),
        Some(json!({"window_id": x.to_string(), "window_title": "Kitchen"}))
    );
}

#[test]
fn a_block_moved_to_another_window_is_told_to_both() {
    let (w, x) = (Id::next(), Id::next());
    let (t, u, v) = (Id::next(), Id::next(), Id::next());
    let kitchen = |blocks| WindowSeen {
        title: "Kitchen".into(),
        ..window(x, blocks)
    };
    let before = app(vec![
        window(w, vec![block(t), opened(block(u))]),
        kitchen(vec![block(v)]),
    ]);
    let after = app(vec![
        window(w, vec![block(t)]),
        kitchen(vec![block(v), opened(block(u))]),
    ]);

    let events = changes(&before, &after);
    assert_eq!(
        kinds(&events),
        [
            "window.layout_changed",
            "block.moved",
            "window.layout_changed"
        ],
        "neither closed nor made, nor opened again"
    );
    let moved = &events[1];
    assert_eq!(
        moved.payload,
        json!({
            "window_id": x.to_string(),
            "window_title": "Kitchen",
            "block_id": u.to_string(),
            "block_title": "clip.mp4",
            "from_window_id": w.to_string(),
            "from_window_title": "Plays",
        })
    );
    assert!(moved.heard_in(Scope::Window(w)) && moved.heard_in(Scope::Window(x)));
    assert!(!moved.heard_in(Scope::App));

    // Its window closing as it goes, the rest close with it.
    let gone = app(vec![kitchen(vec![block(v), opened(block(u)), block(t)])]);
    let events = changes(&after, &gone);
    assert_eq!(
        kinds(&events),
        [
            "block.moved",
            "window.layout_changed",
            "window.closed",
            "window.active_changed"
        ],
        "{:?}",
        said(&events)
    );
    let left = app(vec![kitchen(vec![block(v), opened(block(u))])]);
    assert_eq!(
        kinds(&changes(&after, &left)),
        ["block.closed", "window.closed", "window.active_changed"],
        "one not moved is closed"
    );
}

#[test]
fn how_a_file_plays_is_told_once_it_has_opened() {
    let t = Id::next();
    let opening = block(t);
    let playing = BlockSeen {
        playing: true,
        ..opening.clone()
    };
    assert!(
        block_changes(&opening, &playing).is_empty(),
        "nothing plays till it has opened"
    );

    let open = opened(playing);
    assert_eq!(
        said(&block_changes(&opening, &open)),
        [
            (
                "playback.opened",
                json!({"title": "clip.mp4", "duration": 10.0})
            ),
            ("playback.playing", at(0.0, 0.0, 0)),
        ]
    );

    // Where it went, though it shows from the frame before.
    let sought = BlockSeen {
        seeks: 1,
        sought: 5.0,
        position: 4.96,
        ..open.clone()
    };
    assert_eq!(
        said(&block_changes(&open, &sought)),
        [("playback.seeked", at(0.5, 5.0, 125))]
    );

    let ended = BlockSeen {
        playing: false,
        ended: true,
        position: 10.0,
        ..sought.clone()
    };
    assert_eq!(
        said(&block_changes(&sought, &ended)),
        [("playback.ended", at(1.0, 10.0, 250))],
        "not paused"
    );
    let again = BlockSeen {
        playing: true,
        ended: false,
        position: 0.0,
        ..ended.clone()
    };
    assert_eq!(
        said(&block_changes(&ended, &again)),
        [("playback.playing", at(0.0, 0.0, 0))]
    );
    let paused = BlockSeen {
        playing: false,
        position: 1.0,
        ..again.clone()
    };
    assert_eq!(
        said(&block_changes(&again, &paused)),
        [("playback.paused", at(0.1, 1.0, 25))]
    );

    let mut changed = BlockSeen {
        volume: 50.0,
        muted: true,
        chapter: Some(2),
        ..paused.clone()
    };
    changed.tracks[1] = Some(2);
    changed.tracks[2] = Some(1);
    assert_eq!(
        said(&block_changes(&paused, &changed)),
        [
            (
                "audio.volume_changed",
                json!({"volume": 50.0, "muted": true})
            ),
            (
                "playback.track_changed",
                json!({"kind": "audio", "track": 2})
            ),
            (
                "playback.track_changed",
                json!({"kind": "subtitles", "track": 1})
            ),
            ("playback.chapter_changed", json!({"chapter": 2})),
        ]
    );
    assert!(block_changes(&changed, &changed).is_empty());
}

#[test]
fn files_dropped_together_are_each_told_opening_and_playing_however_looks_fall() {
    let w = Id::next();
    let ids = [Id::next(), Id::next(), Id::next()];
    let playing = |id| {
        opened(BlockSeen {
            playing: true,
            ..block(id)
        })
    };
    let each: Vec<(&str, Value)> = ids
        .iter()
        .flat_map(|id| {
            [
                ("playback.opened", json!(id.to_string())),
                ("playback.playing", json!(id.to_string())),
            ]
        })
        .collect();
    let opening = |events: &[Event]| -> Vec<(&str, Value)> {
        events
            .iter()
            .filter(|event| matches!(event.kind, "playback.opened" | "playback.playing"))
            .map(|event| (event.kind, event.payload["block_id"].clone()))
            .collect()
    };
    let empty = app(vec![window(w, Vec::new())]);
    let all = app(vec![window(w, ids.map(playing).to_vec())]);

    // All three open and play before the next look.
    let events = changes(&empty, &all);
    assert_eq!(
        kinds(&events),
        [
            "block.created",
            "block.created",
            "block.created",
            "window.layout_changed",
            "block.active_changed",
            "playback.opened",
            "playback.playing",
            "playback.opened",
            "playback.playing",
            "playback.opened",
            "playback.playing",
        ]
    );
    assert_eq!(opening(&events), each);

    // Looks between: each told as it opens, none twice, though each says
    // it plays before it has opened, as a file played on dropping does.
    let starting = |id| BlockSeen {
        playing: true,
        ..block(id)
    };
    let made = app(vec![window(w, ids.map(starting).to_vec())]);
    let first = app(vec![window(
        w,
        vec![playing(ids[0]), starting(ids[1]), starting(ids[2])],
    )]);
    let mut told = opening(&changes(&empty, &made));
    told.extend(opening(&changes(&made, &first)));
    told.extend(opening(&changes(&first, &all)));
    assert_eq!(told, each);
}

#[test]
fn where_a_file_plays_is_told_from_0_to_1_in_seconds_and_by_its_frame() {
    assert_eq!(
        playhead(2.5, Some(10.0), Some(25.0)),
        at(0.25, 2.5, 62),
        "the frame showing, not the next"
    );
    // A frame's own time is that frame's, though multiplied back it can
    // fall a hair short.
    for fps in [25.0, 24000.0 / 1001.0, 30000.0 / 1001.0] {
        for frame in 0..10_000_u32 {
            let time = f64::from(frame) / fps;
            assert_eq!(
                playhead(time, None, Some(fps))["frame"],
                json!(frame),
                "{time} at {fps}"
            );
        }
    }
    assert_eq!(
        playhead(3.0, None, Some(25.0)),
        json!({"position": null, "time": 3.0, "frame": 75}),
        "a live stream has no end to be through to"
    );
    assert_eq!(
        playhead(3.0, Some(0.0), Some(25.0))["position"],
        Value::Null
    );
    assert_eq!(
        playhead(3.0, Some(12.0), None),
        json!({"position": 0.25, "time": 3.0, "frame": null}),
        "sound alone has no frames"
    );
    assert_eq!(playhead(3.0, Some(12.0), Some(0.0))["frame"], Value::Null);
    // A device cast to may say it's past either end.
    assert_eq!(
        playhead(10.5, Some(10.0), Some(25.0))["position"],
        json!(1.0)
    );
    assert_eq!(
        playhead(-0.5, Some(10.0), Some(25.0)),
        json!({"position": 0.0, "time": -0.5, "frame": 0})
    );
}

#[test]
fn a_file_opened_in_place_of_another_is_told_as_it_opens_or_fails() {
    let first = BlockSeen {
        playing: true,
        volume: 50.0,
        ..opened(block(Id::next()))
    };
    // The next one waits to be known, then starts out as the first was.
    let next = BlockSeen {
        opening: 1,
        opened: None,
        playing: false,
        ..first.clone()
    };
    assert!(block_changes(&first, &next).is_empty());
    let next_open = BlockSeen {
        opened: Some(json!({"title": "next.mp4"})),
        playing: true,
        ..next.clone()
    };
    assert_eq!(
        said(&block_changes(&next, &next_open)),
        [
            ("playback.opened", json!({"title": "next.mp4"})),
            ("playback.playing", at(0.0, 0.0, 0)),
        ],
        "its volume as it was, and no seek"
    );
    // Straight from one to the next.
    assert_eq!(
        kinds(&block_changes(&first, &next_open)),
        ["playback.opened", "playback.playing"]
    );

    let missing = BlockSeen {
        opening: 2,
        opened: None,
        failed: Some("no such file".into()),
        playing: false,
        ..next_open.clone()
    };
    assert_eq!(
        said(&block_changes(&next_open, &missing)),
        [("playback.open_failed", json!({"error": "no such file"}))]
    );
    assert!(block_changes(&missing, &missing).is_empty());

    let broken = BlockSeen {
        failed: Some("the connection dropped".into()),
        ..next_open.clone()
    };
    assert_eq!(
        said(&block_changes(&next_open, &broken)),
        [(
            "playback.failed",
            json!({"error": "the connection dropped"})
        )]
    );
}

#[test]
fn casts_and_picture_in_picture_are_told_as_they_start_and_end() {
    let here = opened(block(Id::next()));
    let tv = json!({"id": "tv", "kind": "Chromecast", "name": "Living Room"});
    let speaker = json!({"id": "sp", "kind": "AirPlay", "name": "Kitchen"});
    let on_tv = BlockSeen {
        casting: Some(tv.clone()),
        ..here.clone()
    };
    assert_eq!(
        said(&block_changes(&here, &on_tv)),
        [("cast.started", json!({"device": tv}))]
    );
    let on_speaker = BlockSeen {
        casting: Some(speaker.clone()),
        ..here.clone()
    };
    assert_eq!(
        said(&block_changes(&on_tv, &on_speaker)),
        [
            ("cast.ended", json!({"device": tv})),
            ("cast.started", json!({"device": speaker})),
        ]
    );
    // It goes on casting the next file.
    let next = BlockSeen {
        opening: 1,
        ..on_speaker.clone()
    };
    assert_eq!(
        kinds(&block_changes(&on_speaker, &next)),
        ["playback.opened"]
    );
    let back = BlockSeen {
        casting: None,
        ..next.clone()
    };
    assert_eq!(
        said(&block_changes(&next, &back)),
        [("cast.ended", json!({"device": speaker}))]
    );

    let pip = BlockSeen {
        picture_in_picture: true,
        ..here.clone()
    };
    assert_eq!(
        said(&block_changes(&here, &pip)),
        [(
            "block.picture_in_picture_changed",
            json!({"picture_in_picture": true})
        )]
    );
    assert_eq!(
        said(&block_changes(&pip, &here)),
        [(
            "block.picture_in_picture_changed",
            json!({"picture_in_picture": false})
        )]
    );
}

#[test]
fn the_apps_outputs_sound_and_devices_are_its_own() {
    let w = Id::next();
    let before = app(vec![window(w, Vec::new())]);
    let after = Seen {
        outputs: json!([{"id": "1", "name": "Speakers", "kind": null}]),
        route: json!({"waiting": "Headphones"}),
        devices: json!({"Chromecast": [{"id": "tv", "name": "Living Room", "shows": "video"}]}),
        ..before.clone()
    };
    let events = changes(&before, &after);
    assert_eq!(
        said(&events),
        [
            (
                "output.devices_changed",
                json!({"outputs": [{"id": "1", "name": "Speakers", "kind": null}]})
            ),
            (
                "output.route_changed",
                json!({"route": {"waiting": "Headphones"}})
            ),
            (
                "cast.devices_changed",
                json!({"devices": {"Chromecast": [
                    {"id": "tv", "name": "Living Room", "shows": "video"}
                ]}})
            ),
        ]
    );
    for event in &events {
        assert!(event.heard_in(Scope::App) && event.heard_in(Scope::All));
        assert!(!event.heard_in(Scope::Window(w)));
    }
}

#[test]
fn each_scope_hears_its_own() {
    let (w, x, t) = (Id::next(), Id::next(), Id::next());
    let created = app(vec![window(w, vec![block(t)])]);
    let events = changes(&Seen::default(), &created);
    let heard = |scope| -> Vec<&str> {
        events
            .iter()
            .filter(|event| event.heard_in(scope))
            .map(|event| event.kind)
            .collect()
    };
    assert_eq!(
        heard(Scope::App),
        ["window.created", "window.active_changed"]
    );
    assert_eq!(
        heard(Scope::Window(w)),
        [
            "window.created",
            "block.created",
            "window.layout_changed",
            "block.active_changed",
            "window.active_changed",
        ]
    );
    assert!(heard(Scope::Window(x)).is_empty());
    assert_eq!(heard(Scope::All).len(), events.len());
}
