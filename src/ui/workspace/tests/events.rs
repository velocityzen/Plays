//! The events told for what windows and blocks do, as the UI and commands
//! do it: each once, in order.

use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use serde_json::{Value, json};

use super::open;
use crate::api::events::{self, Event, LOOK_EVERY, POSITION_EVERY, Scope};
use crate::api::{self, Id, Invocation};
use crate::app::AudioEngine;
use crate::cast::fake::FakeTarget;
use crate::cast::{Casting, Shows};
use crate::media::output::DeviceInfo;
use crate::ui::testing::{self, click, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

fn listen(cx: &mut VisualTestContext, scope: Scope, positions: bool) -> UnboundedReceiver<Event> {
    TestAppContext::update(cx, |cx| events::listen(scope, positions, cx))
}

/// Look now for what changed, as the app does every `LOOK_EVERY`.
fn look(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    TestAppContext::update(cx, events::look);
}

/// What `listener` has been told since last asked, by type.
fn kinds(listener: &mut UnboundedReceiver<Event>) -> Vec<&'static str> {
    told(listener).iter().map(|event| event.kind).collect()
}

fn told(listener: &mut UnboundedReceiver<Event>) -> Vec<Event> {
    std::iter::from_fn(|| listener.try_recv().ok()).collect()
}

/// Run `invocation` as a deferred command does, outside the window's
/// update; what it changed is looked for once it has settled.
fn run(cx: &mut VisualTestContext, invocation: Invocation) -> Value {
    let done = TestAppContext::update(cx, |cx| api::run(invocation, cx)).unwrap();
    cx.run_until_parked();
    done
}

/// The app's clock, a step on: the app ticks its players, which play on
/// once they have what they need, and looks for what changed.
const STEP: Duration = Duration::from_millis(100);

/// Let what plays play on until `done`, the app's clock moving with the
/// time, a `STEP` at a time; whether it was done in time.
fn play_until(
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) -> bool {
    for _ in 0..100 {
        cx.executor().advance_clock(STEP);
        cx.run_until_parked();
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn window_id(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Id {
    window.read_with(cx, |workspace, _| workspace.id()).unwrap()
}

fn block_id(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Id {
    window
        .read_with(cx, |workspace, cx| workspace.blocks[0].view.read(cx).id())
        .unwrap()
}

#[gpui::test]
fn a_block_opened_renamed_and_its_window_full_screen_are_told_once_in_order(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    let w = window_id(window, &cx);
    let mut all = listen(&mut cx, Scope::All, false);
    let mut app = listen(&mut cx, Scope::App, false);

    // Dropped on the window, as Finder does.
    open(window, &mut cx, &["clip.mp4"], true);
    look(&mut cx);
    let t = block_id(window, &cx);
    let opened = told(&mut all);
    assert_eq!(
        opened.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            "block.created",
            "window.layout_changed",
            "block.active_changed",
            "playback.opened",
            "playback.playing",
        ]
    );
    assert_eq!(opened[3].payload["block_id"], json!(t.to_string()));
    assert_eq!(opened[3].payload["title"], json!("clip.mp4"));
    assert!(opened[3].payload["duration"].as_f64().unwrap() > 0.0);
    assert!(kinds(&mut app).is_empty(), "a window's own");
    look(&mut cx);
    assert!(kinds(&mut all).is_empty(), "told once");

    run(
        &mut cx,
        Invocation::new("block.rename").with_args(json!({"label": "left"})),
    );
    run(
        &mut cx,
        Invocation::new("window.rename").with_args(json!({"label": "news"})),
    );
    run(&mut cx, Invocation::new("window.toggle_full_screen"));
    let said: Vec<(&str, Value)> = told(&mut all)
        .into_iter()
        .map(|event| (event.kind, event.payload))
        .collect();
    let (w, t) = (w.to_string(), t.to_string());
    assert_eq!(
        said,
        [
            (
                "app.command_run",
                json!({"command": "block.rename", "source": "ui", "args": {"label": "left"}})
            ),
            (
                "block.label_changed",
                json!({
                    "window_id": w,
                    "window_title": "Plays",
                    "block_id": t,
                    "block_title": "left",
                    "label": "left",
                })
            ),
            (
                "app.command_run",
                json!({"command": "window.rename", "source": "ui", "args": {"label": "news"}})
            ),
            (
                "window.label_changed",
                json!({"window_id": w, "window_title": "news", "label": "news"})
            ),
            (
                "app.command_run",
                json!({"command": "window.toggle_full_screen", "source": "ui"})
            ),
            (
                "window.full_screen_changed",
                json!({"window_id": w, "window_title": "news", "full_screen": true})
            ),
        ]
    );
    assert_eq!(
        kinds(&mut app),
        [
            "app.command_run",
            "app.command_run",
            "window.label_changed",
            "app.command_run"
        ],
        "the app's: commands, and windows' names"
    );
    run(&mut cx, Invocation::new("window.toggle_full_screen"));
    assert_eq!(
        kinds(&mut all),
        ["app.command_run", "window.full_screen_changed"]
    );
    assert_eq!(kinds(&mut app), ["app.command_run"]);
}

#[gpui::test]
fn a_block_moved_to_a_window_of_its_own_and_the_window_closed_are_told_in_order(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], true);
    let (w, t) = (window_id(window, &cx), block_id(window, &cx));
    let mut all = listen(&mut cx, Scope::All, false);
    let mut app = listen(&mut cx, Scope::App, false);
    let mut first = listen(&mut cx, Scope::Window(w), false);

    // To a window of its own, where it plays on.
    run(&mut cx, Invocation::new("block.move_to_new_window"));
    let moved = told(&mut all);
    assert_eq!(
        moved.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            "app.command_run",
            "window.created",
            "window.layout_changed",
            "block.active_changed",
            "block.moved",
            "window.layout_changed",
            "block.active_changed",
        ]
    );
    let x = moved[1].payload["window_id"].clone();
    assert_ne!(x, json!(w.to_string()));
    assert_eq!(
        moved[4].payload,
        json!({
            "window_id": x,
            "window_title": "Plays",
            "block_id": t.to_string(),
            "block_title": "clip.mp4",
            "from_window_id": w.to_string(),
            "from_window_title": "Plays",
        })
    );
    assert_eq!(kinds(&mut app), ["app.command_run", "window.created"]);
    assert_eq!(
        kinds(&mut first),
        [
            "window.layout_changed",
            "block.active_changed",
            "block.moved"
        ],
        "the window it left hears it go"
    );

    let x = x.as_str().unwrap();
    run(&mut cx, Invocation::new("window.focus").in_window(x));
    let front = told(&mut app);
    assert_eq!(front.len(), 2, "{front:?}");
    assert_eq!(front[1].kind, "window.active_changed");
    assert_eq!(
        front[1].payload,
        json!({"window_id": x, "window_title": "Plays"})
    );
    assert_eq!(
        kinds(&mut all),
        ["app.command_run", "window.active_changed"]
    );
    assert_eq!(
        kinds(&mut first),
        Vec::<&str>::new(),
        "another window in front isn't this one's"
    );

    // Closed, the one before it is in front again.
    run(&mut cx, Invocation::new("window.close").in_window(x));
    look(&mut cx);
    assert_eq!(
        kinds(&mut all),
        [
            "app.command_run",
            "block.closed",
            "window.closed",
            "window.active_changed",
        ]
    );
    let closed = told(&mut app);
    assert_eq!(
        closed.iter().map(|event| event.kind).collect::<Vec<_>>(),
        ["app.command_run", "window.closed", "window.active_changed"]
    );
    assert_eq!(
        closed[2].payload,
        json!({"window_id": w.to_string(), "window_title": "Plays"})
    );
    assert_eq!(kinds(&mut first), ["window.active_changed"]);
}

#[gpui::test]
fn a_block_maximized_is_told_as_the_layout_changes(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-chapters.mkv"], false);
    let w = window_id(window, &cx);
    let mut heard = listen(&mut cx, Scope::Window(w), false);
    let t = block_id(window, &cx).to_string();

    run(&mut cx, Invocation::new("block.zoom").on_block("1"));
    let zoomed = told(&mut heard);
    let layout: Vec<&Event> = zoomed
        .iter()
        .filter(|event| event.kind == "window.layout_changed")
        .collect();
    assert_eq!(layout.len(), 1, "{zoomed:?}");
    assert_eq!(layout[0].payload["maximized"], json!(t));

    run(&mut cx, Invocation::new("block.zoom").on_block("1"));
    let back = told(&mut heard);
    assert_eq!(
        back.iter().map(|event| event.kind).collect::<Vec<_>>(),
        ["window.layout_changed"]
    );
    assert_eq!(back[0].payload["maximized"], Value::Null);
}

#[gpui::test]
fn a_file_set_to_play_is_told_playing_once_it_moves_where_it_is_then(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let w = window_id(window, &cx);
    let mut heard = listen(&mut cx, Scope::Window(w), false);
    // Sought, and set to play: nothing to play from yet, as it isn't
    // ticked till asked.
    run(
        &mut cx,
        Invocation::new("playback.seek").with_args(json!({"time": 1.0})),
    );
    run(&mut cx, Invocation::new("playback.play"));
    look(&mut cx);
    assert_eq!(kinds(&mut heard), ["playback.seeked"]);
    let inspected = run(&mut cx, Invocation::new("block.inspect"));
    assert_eq!(
        (&inspected["playing"], &inspected["buffering"]),
        (&json!(true), &json!(true)),
        "set to play, and waiting"
    );

    let started = wait_until(&mut cx, |cx| testing::all_started(window, 1, cx));
    assert!(started, "it moved");
    look(&mut cx);
    let told = told(&mut heard);
    assert_eq!(
        told.iter().map(|event| event.kind).collect::<Vec<_>>(),
        ["playback.playing"]
    );
    let time = told[0].payload["time"].as_f64().unwrap();
    assert!(
        (1.0..1.1).contains(&time),
        "from where it was sought: {time}"
    );
}

#[gpui::test]
fn how_a_file_plays_is_told_for_its_controls_and_commands(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-chapters.mkv"], true);
    let w = window_id(window, &cx);
    let mut heard = listen(&mut cx, Scope::Window(w), false);
    let mut elsewhere = listen(&mut cx, Scope::Window(Id::next()), false);

    run(&mut cx, Invocation::new("playback.play_pause"));
    assert_eq!(
        kinds(&mut heard),
        ["playback.paused"],
        "not the command: the app's"
    );
    // Its play button sends the command once the click is handled.
    click(&mut cx, "play");
    look(&mut cx);
    assert_eq!(kinds(&mut heard), ["playback.playing"]);
    run(&mut cx, Invocation::new("playback.play_pause"));
    kinds(&mut heard);

    run(
        &mut cx,
        Invocation::new("playback.seek").with_args(json!({"time": 0.5})),
    );
    let sought = told(&mut heard);
    assert_eq!(sought.len(), 1);
    assert_eq!(sought[0].kind, "playback.seeked");
    // Its time in seconds, how far through from 0 to 1, and the frame at
    // 25 a second.
    assert_eq!(sought[0].payload["time"], json!(0.5));
    assert_eq!(sought[0].payload["position"], json!(0.5 / 2.021));
    assert_eq!(sought[0].payload["frame"], json!(12));
    // A step back from there goes to the start; a frame on, once one
    // shows, past it.
    run(&mut cx, Invocation::new("playback.seek_backward"));
    let back = told(&mut heard);
    assert_eq!(back.len(), 1, "{back:?}");
    let start = json!({"position": 0.0, "time": 0.0, "frame": 0});
    assert_eq!(back[0].payload["position"], start["position"]);
    assert_eq!(back[0].payload["time"], start["time"]);
    assert_eq!(back[0].payload["frame"], start["frame"]);
    let shown = play_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace.blocks[0]
                    .view
                    .read(cx)
                    .player()
                    .frame_pts()
                    .is_some()
            })
            .unwrap()
    });
    assert!(shown, "no frame showed");
    run(&mut cx, Invocation::new("playback.next_frame"));
    let stepped = told(&mut heard);
    assert_eq!(
        stepped.iter().map(|event| event.kind).collect::<Vec<_>>(),
        ["playback.seeked"]
    );
    assert_eq!(stepped[0].payload["frame"], json!(1), "{stepped:?}");
    assert!(stepped[0].payload["position"].as_f64().unwrap() > 0.0);

    run(
        &mut cx,
        Invocation::new("audio.set_volume").with_args(json!({"volume": 40})),
    );
    run(&mut cx, Invocation::new("audio.toggle_mute"));
    let volumes: Vec<Value> = told(&mut heard)
        .into_iter()
        .map(|event| {
            assert_eq!(event.kind, "audio.volume_changed");
            json!([event.payload["volume"], event.payload["muted"]])
        })
        .collect();
    assert_eq!(volumes, [json!([40.0, false]), json!([40.0, true])]);

    run(
        &mut cx,
        Invocation::new("playback.go_to_chapter").with_args(json!({"number": 2})),
    );
    let chapter = told(&mut heard);
    assert_eq!(
        chapter.iter().map(|event| event.kind).collect::<Vec<_>>(),
        ["playback.seeked", "playback.chapter_changed"]
    );
    assert_eq!(chapter[1].payload["chapter"], json!(2));
    assert!(
        kinds(&mut elsewhere).is_empty(),
        "another window's hear none"
    );
}

/// Played to its end, a file says so, not that it paused.
#[gpui::test]
fn a_file_played_to_its_end_is_told_ended_not_paused(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-chapters.mkv"], true);
    let w = window_id(window, &cx);
    let mut heard = listen(&mut cx, Scope::Window(w), false);
    run(&mut cx, Invocation::new("playback.play_pause"));
    assert_eq!(kinds(&mut heard), ["playback.paused"]);

    let duration = run(&mut cx, Invocation::new("block.inspect"))["duration"]
        .as_f64()
        .unwrap();
    run(
        &mut cx,
        Invocation::new("playback.seek").with_args(json!({"time": duration - 0.2})),
    );
    run(&mut cx, Invocation::new("playback.play_pause"));
    let ended = play_until(&mut cx, |cx| {
        let inspected =
            TestAppContext::update(cx, |cx| api::run(Invocation::new("block.inspect"), cx));
        inspected.unwrap()["ended"] == json!(true)
    });
    assert!(ended, "it didn't play to its end");
    look(&mut cx);
    assert_eq!(
        kinds(&mut heard),
        [
            "playback.seeked",
            "playback.chapter_changed",
            "playback.playing",
            "playback.ended"
        ]
    );
}

#[gpui::test]
fn positions_are_told_only_to_those_who_ask_once_a_second_at_most(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], true);
    let mut asked = listen(&mut cx, Scope::All, true);
    let mut not_asked = listen(&mut cx, Scope::All, false);
    // Each in seconds, with how far through it is and the frame.
    let positions = |listener: &mut UnboundedReceiver<Event>| -> Vec<f64> {
        told(listener)
            .into_iter()
            .filter(|event| event.kind == "playback.position")
            .map(|event| {
                let payload = &event.payload;
                let time = payload["time"].as_f64().unwrap();
                assert_eq!(payload["duration"], json!(2.0), "{payload}");
                assert_eq!(payload["position"], json!(time / 2.0), "{payload}");
                assert_eq!(payload["frame"], json!((time * 25.0) as u64), "{payload}");
                time
            })
            .collect()
    };

    look(&mut cx);
    let first = positions(&mut asked);
    assert_eq!(first.len(), 1, "where it is, to start with");
    std::thread::sleep(Duration::from_millis(50));
    look(&mut cx);
    assert!(positions(&mut asked).is_empty(), "not again so soon");

    // As it plays, the app looking on its own: each a second or more
    // after the last, on the app's clock.
    let mut at = Vec::new();
    let mut steps = 0;
    let played = play_until(&mut cx, |_| {
        steps += 1;
        for position in positions(&mut asked) {
            at.push((STEP * steps, position));
        }
        at.len() == 2
    });
    assert!(played, "told {at:?}");
    assert!(
        at[0].1 > first[0] && at[1].1 > at[0].1,
        "{first:?}, then {at:?}"
    );
    assert!(at[0].0 >= POSITION_EVERY, "{at:?}");
    assert!(at[1].0 - at[0].0 >= POSITION_EVERY, "{at:?}");
    assert!(positions(&mut not_asked).is_empty());

    // Paused, it stays where it is: told once more at most.
    run(&mut cx, Invocation::new("playback.play_pause"));
    let mut after = Vec::new();
    play_until(&mut cx, |_| {
        after.extend(positions(&mut asked));
        false
    });
    assert!(after.len() <= 1, "{after:?}");
}

#[gpui::test]
fn sound_alone_is_told_with_no_frame(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tone.m4a"], false);
    let mut heard = listen(&mut cx, Scope::All, false);
    run(
        &mut cx,
        Invocation::new("playback.seek").with_args(json!({"time": 0.5})),
    );
    let sought = told(&mut heard);
    let sought = sought
        .iter()
        .find(|event| event.kind == "playback.seeked")
        .unwrap();
    assert_eq!(sought.payload["time"], json!(0.5));
    assert!(sought.payload["position"].as_f64().unwrap() > 0.0);
    assert_eq!(sought.payload["frame"], Value::Null, "{sought:?}");
}

#[gpui::test]
fn outputs_devices_and_casts_are_told_as_found_and_started(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let fake = FakeTarget::default();
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    open(window, &mut cx, &["clip.mp4"], true);
    let mut all = listen(&mut cx, Scope::All, false);

    let tv = fake.add("Living Room", Shows::Video);
    cx.update(|_, cx| {
        Casting::refresh(cx);
        AudioEngine::set_devices(
            vec![DeviceInfo {
                id: "speakers".into(),
                name: "Speakers".into(),
                bluetooth: false,
            }],
            cx,
        );
    });
    cx.executor().advance_clock(LOOK_EVERY);
    cx.run_until_parked();
    let found = told(&mut all);
    assert_eq!(
        found.iter().map(|event| event.kind).collect::<Vec<_>>(),
        ["output.devices_changed", "cast.devices_changed"]
    );
    assert_eq!(
        found[0].payload["outputs"],
        json!([{"id": "speakers", "name": "Speakers", "kind": null}])
    );
    assert_eq!(
        found[1].payload["devices"][tv.kind][0]["name"],
        json!("Living Room")
    );

    run(
        &mut cx,
        Invocation::new("cast.to").with_args(json!({"device": tv.id})),
    );
    let started = told(&mut all);
    let kinds_of = |events: &[Event]| events.iter().map(|event| event.kind).collect::<Vec<_>>();
    assert!(
        kinds_of(&started).contains(&"cast.started"),
        "{:?}",
        kinds_of(&started)
    );
    let cast = started
        .iter()
        .find(|event| event.kind == "cast.started")
        .unwrap();
    assert_eq!(cast.payload["device"]["name"], json!("Living Room"));

    run(&mut cx, Invocation::new("cast.stop"));
    let ended = told(&mut all);
    assert_eq!(
        ended
            .iter()
            .filter(|event| event.kind.starts_with("cast."))
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        ["cast.ended"]
    );
}

#[gpui::test]
fn tracks_and_a_file_opened_in_place_of_another_are_told_as_they_change(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-subs.mkv"], false);
    let mut heard = listen(&mut cx, Scope::All, false);
    let subtitles = |number: Option<u64>| {
        Invocation::new("subtitles.select_track").with_args(match number {
            Some(number) => json!({"number": number}),
            None => json!({}),
        })
    };

    run(&mut cx, subtitles(None));
    run(&mut cx, subtitles(Some(1)));
    run(&mut cx, subtitles(None));
    let tracks: Vec<Value> = told(&mut heard)
        .into_iter()
        .filter(|event| event.kind == "playback.track_changed")
        .map(|event| json!([event.payload["kind"], event.payload["track"]]))
        .collect();
    assert!(
        tracks.ends_with(&[json!(["subtitles", 1]), json!(["subtitles", null])]),
        "{tracks:?}"
    );
    assert!(tracks.len() <= 3, "{tracks:?}");

    // The next file in the folder, in the same block: it opens, as itself.
    run(&mut cx, Invocation::new("playback.next_file"));
    let inspect = |cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| api::run(Invocation::new("block.inspect"), cx)).unwrap()
    };
    let next = play_until(&mut cx, |cx| {
        let inspected = inspect(cx);
        inspected["title"] != json!("clip-subs.mkv") && inspected["duration"].is_number()
    });
    assert!(next, "the next file didn't open");
    look(&mut cx);
    let opened = told(&mut heard);
    let kinds: Vec<&str> = opened.iter().map(|event| event.kind).collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|&&kind| kind == "playback.opened")
            .count(),
        1,
        "{kinds:?}"
    );
    let at = kinds
        .iter()
        .position(|&kind| kind == "playback.opened")
        .unwrap();
    assert_eq!(opened[at].payload["title"], json!("clip.mp4"));
    assert!(
        !kinds.contains(&"playback.track_changed") && !kinds.contains(&"block.created"),
        "the same block, its tracks as the file has them: {kinds:?}"
    );

    // Opened in place by no command, and known before the app looks: told
    // as another file still.
    let view = window
        .read_with(&cx, |workspace, _| workspace.blocks[0].view.clone())
        .unwrap();
    view.update(&mut cx, |view, cx| view.open_neighbor(-1, cx));
    let back = wait_until(&mut cx, |cx| {
        let inspected = cx
            .update(|cx| api::run(Invocation::new("block.inspect"), cx))
            .unwrap();
        inspected["title"] == json!("clip-subs.mkv") && inspected["duration"].is_number()
    });
    assert!(back, "the file before didn't open");
    look(&mut cx);
    let opened: Vec<Event> = told(&mut heard)
        .into_iter()
        .filter(|event| event.kind == "playback.opened")
        .collect();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert_eq!(opened[0].payload["title"], json!("clip-subs.mkv"));
}

#[gpui::test]
fn a_file_that_cant_open_is_told_so_and_where_it_is_isnt(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let mut heard = listen(&mut cx, Scope::All, true);
    let broken = std::env::temp_dir().join(format!("plays-broken-{}.mp4", Id::next()));
    std::fs::write(&broken, b"not a video, nor anything else").unwrap();

    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![broken.clone()], cx);
        })
        .unwrap();
    let failed = play_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace
                    .blocks
                    .first()
                    .is_some_and(|block| block.view.read(cx).player().error().is_some())
            })
            .unwrap()
    });
    std::fs::remove_file(&broken).ok();
    assert!(failed, "it opened");
    look(&mut cx);
    let events = told(&mut heard);
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            "block.created",
            "window.layout_changed",
            "block.active_changed",
            "playback.open_failed",
        ]
    );
    assert!(!events[3].payload["error"].as_str().unwrap().is_empty());
}

#[gpui::test]
fn as_the_app_quits_each_window_is_told_closing_then_quit_to_everyone(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    // No one listening, nothing to tell.
    TestAppContext::update(&cx, events::quit);
    open(window, &mut cx, &["clip.mp4"], true);
    let (w, t) = (window_id(window, &cx), block_id(window, &cx));
    let mut all = listen(&mut cx, Scope::All, false);
    let mut app = listen(&mut cx, Scope::App, false);
    let mut first = listen(&mut cx, Scope::Window(w), false);
    // Another, not looked at yet: told opening, then closing.
    let other = cx
        .update(|_, cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    cx.run_until_parked();
    let x = window_id(other, &cx);
    let mut second = listen(&mut cx, Scope::Window(x), false);

    TestAppContext::update(&cx, events::quit);
    let quitting = told(&mut all);
    assert_eq!(
        quitting.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            "window.created",
            "block.closed",
            "window.closed",
            "window.closed",
            "window.active_changed",
            "app.quit",
        ]
    );
    let w = w.to_string();
    assert_eq!(
        quitting[1].payload,
        json!({
            "window_id": w,
            "window_title": "Plays",
            "block_id": t.to_string(),
            "block_title": "clip.mp4",
        })
    );
    assert_eq!(
        quitting[3].payload,
        json!({"window_id": x.to_string(), "window_title": "Plays"})
    );
    assert_eq!(
        quitting[4].payload,
        json!({"window_id": null, "window_title": null}),
        "none in front"
    );
    assert_eq!(quitting[5].payload, json!({}));
    assert_eq!(
        kinds(&mut app),
        [
            "window.created",
            "window.closed",
            "window.closed",
            "window.active_changed",
            "app.quit",
        ]
    );
    assert_eq!(
        kinds(&mut first),
        ["block.closed", "window.closed", "app.quit"],
        "its own closing, and quit"
    );
    assert_eq!(
        kinds(&mut second),
        ["window.created", "window.closed", "app.quit"]
    );
}
