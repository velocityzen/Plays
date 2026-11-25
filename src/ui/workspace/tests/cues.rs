//! Cues: a block reaching a time or a frame as it plays, or landing on
//! it, told once to the listener that set it.

use std::time::Duration;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use serde_json::{Value, json};

use super::open;
use crate::api::events::{self, CUE, Event, Scope, cues};
use crate::api::{self, Error, Invocation};
use crate::ui::testing;
use crate::ui::workspace::Workspace;

/// The app's clock, a step on, as `events`' tests move it.
const STEP: Duration = Duration::from_millis(100);

/// A listener to the app's events, which are no block's: what it hears of
/// blocks is its cues.
fn listening(cx: &mut VisualTestContext) -> (UnboundedSender<Event>, UnboundedReceiver<Event>) {
    let (to, heard) = unbounded();
    let sending = to.clone();
    TestAppContext::update(cx, |cx| {
        events::listen_into(Scope::App, false, sending, cx);
    });
    (to, heard)
}

/// Set the cue `name`, told to `to`.
fn cue(
    cx: &mut VisualTestContext,
    name: &str,
    to: &UnboundedSender<Event>,
) -> Result<Value, Error> {
    TestAppContext::update(cx, |cx| cues::cue(name, to, cx))
}

/// The cues told to `heard` since last asked: each one's name and payload.
fn told(heard: &mut UnboundedReceiver<Event>) -> Vec<(String, Value)> {
    std::iter::from_fn(|| heard.try_recv().ok())
        .filter(|event| event.kind == CUE)
        .map(|event| (event.name().to_owned(), event.payload))
        .collect()
}

/// Move the app's clock on a `STEP` at a time, as the block plays, until
/// `done`, for 10 seconds at most.
fn play_until(cx: &mut VisualTestContext, mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        cx.executor().advance_clock(STEP);
        cx.run_until_parked();
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Long enough for a paused block to tick.
fn tick(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(1100));
    cx.run_until_parked();
}

fn run(cx: &mut VisualTestContext, invocation: Invocation) {
    TestAppContext::update(cx, |cx| api::run(invocation, cx)).unwrap();
    cx.run_until_parked();
}

fn seek(cx: &mut VisualTestContext, time: f64) {
    run(
        cx,
        Invocation::new("playback.seek").with_args(json!({"time": time})),
    );
}

fn block_id(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> String {
    window
        .read_with(cx, |workspace, cx| {
            workspace.blocks[0].view.read(cx).id().to_string()
        })
        .unwrap()
}

#[gpui::test]
fn a_cue_is_told_once_as_its_block_plays_to_it(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    // Two seconds, at 25 frames a second.
    open(window, &mut cx, &["clip.mp4"], true);
    let block = block_id(window, &cx);
    let (to, mut heard) = listening(&mut cx);
    let time = format!("block.{block}.position@1s");
    let frame = format!("block.{block}.position@30f");
    let set = cue(&mut cx, &time, &to).unwrap();
    assert_eq!(set["block"], json!(block));
    let window_id = set["window"].clone();
    cue(&mut cx, &frame, &to).unwrap();

    let mut cued = Vec::new();
    let played = play_until(&mut cx, || {
        cued.extend(told(&mut heard));
        cued.len() >= 2
    });
    assert!(played, "{cued:?}");
    play_until(&mut cx, || {
        cued.extend(told(&mut heard));
        false
    });
    let names: Vec<&str> = cued.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, [&time, &frame], "each once, the time's first");
    for ((_, payload), (least, first)) in cued.iter().zip([(1.0, 25), (1.2, 30)]) {
        assert_eq!(payload["block_id"], json!(block));
        assert_eq!(payload["window_id"], window_id);
        assert_eq!(payload["block_title"], json!("clip.mp4"));
        assert_eq!(payload["duration"], json!(2.0));
        let at = payload["time"].as_f64().unwrap();
        assert!((least..2.0).contains(&at), "{payload}");
        assert_eq!(payload["position"], json!(at / 2.0));
        assert!(payload["frame"].as_u64().unwrap() >= first, "{payload}");
    }
}

#[gpui::test]
fn a_seek_landing_on_a_cue_reaches_it_and_one_past_it_doesnt(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], true);
    run(&mut cx, Invocation::new("playback.play_pause"));
    seek(&mut cx, 0.2);
    tick(&mut cx);
    let block = block_id(window, &cx);
    let (to, mut heard) = listening(&mut cx);
    let landed = format!("block.{block}.position@1s");
    let played = format!("block.{block}.position@12f");
    cue(&mut cx, &landed, &to).unwrap();
    cue(&mut cx, &played, &to).unwrap();

    seek(&mut cx, 1.5);
    tick(&mut cx);
    assert_eq!(told(&mut heard), [], "past both");
    seek(&mut cx, 1.0);
    tick(&mut cx);
    let cued = told(&mut heard);
    assert_eq!(cued.len(), 1, "{cued:?}");
    assert_eq!(cued[0].0, landed);
    assert_eq!(cued[0].1["time"], json!(1.0));
    assert_eq!(cued[0].1["frame"], json!(25));

    // Back before frame 12, and playing through it.
    seek(&mut cx, 0.2);
    tick(&mut cx);
    assert_eq!(told(&mut heard), []);
    run(&mut cx, Invocation::new("playback.play_pause"));
    let mut cued = Vec::new();
    let reached = play_until(&mut cx, || {
        cued.extend(told(&mut heard));
        !cued.is_empty()
    });
    assert!(reached);
    assert_eq!(cued[0].0, played);
    assert!(cued[0].1["frame"].as_u64().unwrap() >= 12, "{cued:?}");
}

#[gpui::test]
fn a_cue_names_a_block_open_and_a_frame_only_in_one_with_frames(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tone.m4a"], false);
    let block = block_id(window, &cx);
    let (to, _heard) = listening(&mut cx);
    let frame = format!("block.{block}.position@3f");
    let why = cue(&mut cx, &frame, &to).unwrap_err();
    assert!(
        matches!(&why, Error::BadArgs { message, .. } if message.contains("sound alone")),
        "{why:?}"
    );
    cue(&mut cx, &format!("block.{block}.position@0.5s"), &to).unwrap();
    let why = cue(&mut cx, "block.Attic.position@1s", &to).unwrap_err();
    assert_eq!(why, Error::not_found("block", "Attic"));
    let why = cue(&mut cx, "block.Attic.position@1", &to).unwrap_err();
    assert!(matches!(why, Error::BadArgs { .. }), "{why:?}");
}
