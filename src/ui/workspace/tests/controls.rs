//! What a window's controls send: each runs its command through the API,
//! aimed at its own block or window. `api::intercept` notes what's sent
//! without running it, so one block can have every control clicked; what
//! the commands do is tested with them.

use std::time::{Duration, Instant};

use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use serde_json::{Value, json};

use super::open;
use crate::api::{self, Invocation};
use crate::ui::pip;
use crate::ui::testing::{
    self, OPEN_WAIT, click, double_click, drag, hover, press, shows, wait_until,
};
use crate::ui::workspace::Workspace;

/// A stream's URL, at an address kept for examples, with the user and
/// password it's watched with, which the URL sent keeps.
const URL: &str = "rtsp://viewer:secret@192.0.2.1/live";

/// The API's name for block `at` in `window`.
fn block_name(window: WindowHandle<Workspace>, at: usize, cx: &VisualTestContext) -> String {
    window
        .read_with(cx, |workspace, cx| {
            workspace.blocks[at].view.read(cx).id().to_string()
        })
        .unwrap()
}

/// The API's name for `window`.
fn window_name(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> String {
    window
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap()
}

/// What was sent since last asked, once it's been sent.
fn sent(cx: &mut VisualTestContext) -> Vec<Invocation> {
    cx.run_until_parked();
    cx.update(|_, cx| api::recorded(cx))
}

/// The one invocation sent since last asked.
fn sent_one(cx: &mut VisualTestContext, control: &str) -> Invocation {
    let mut sent = sent(cx);
    assert_eq!(sent.len(), 1, "{control} sends one command: {sent:?}");
    sent.remove(0)
}

/// A number in `args`.
fn number(args: &Value, name: &str) -> f64 {
    args[name]
        .as_f64()
        .unwrap_or_else(|| panic!("{name} in {args}"))
}

#[gpui::test]
fn each_of_a_blocks_controls_sends_its_command_to_it(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-subs.mkv"], false);
    let view = window
        .read_with(&cx, |workspace, _| workspace.blocks[0].view.clone())
        .unwrap();
    let neighbors = wait_until(&mut cx, |cx| {
        view.read_with(cx, |view, _| view.knows_neighbors())
    });
    assert!(neighbors, "the files beside it weren't found");
    let duration = view
        .read_with(&cx, |view, _| view.duration())
        .expect("its length");
    let block = block_name(window, 0, &cx);
    let on_block = |command: &str, args: Value| {
        Invocation::new(command)
            .with_args(args)
            .on_block(block.clone())
    };
    cx.update(|_, cx| api::intercept(cx));
    hover(&mut cx, "block-0");

    for (control, command) in [
        ("play", "playback.play_pause"),
        ("skip-back", "playback.seek_backward"),
        ("skip-forward", "playback.seek_forward"),
        ("previous", "playback.previous_file"),
        ("next", "playback.next_file"),
        ("loop", "playback.loop.toggle"),
        ("mute", "audio.mute.toggle"),
        ("maximize", "block.zoom"),
        ("close", "block.close"),
    ] {
        click(&mut cx, control);
        assert_eq!(sent(&mut cx), [on_block(command, Value::Null)], "{control}");
    }
    if pip::supported() {
        // Once it shows a picture, drawn.
        let start = Instant::now();
        while !shows(&mut cx, "pip") && start.elapsed() < OPEN_WAIT {
            std::thread::sleep(Duration::from_millis(5));
        }
        click(&mut cx, "pip");
        assert_eq!(
            sent(&mut cx),
            [on_block("block.picture_in_picture.toggle", Value::Null)]
        );
    }
    double_click(&mut cx, "block-0");
    assert_eq!(
        sent(&mut cx),
        [on_block("block.zoom", Value::Null)],
        "a double click"
    );

    // Pressed and let go in the middle: half way, once let go.
    click(&mut cx, "seek");
    let seek = sent_one(&mut cx, "the seek bar");
    assert_eq!(seek.command, "playback.seek");
    assert_eq!(seek.target.block.as_deref(), Some(block.as_str()));
    let time = number(&seek.args, "time");
    assert!((time / duration - 0.5).abs() < 0.2, "{time} of {duration}");

    click(&mut cx, "volume");
    let volume = sent_one(&mut cx, "the volume slider");
    assert_eq!(volume.command, "audio.set_volume");
    assert_eq!(volume.target.block.as_deref(), Some(block.as_str()));
    let volume = number(&volume.args, "volume");
    assert!((volume - 50.0).abs() < 20.0, "{volume}");

    // Its one audio track, its subtitles, and none: rows by stream.
    for (row, command, number) in [
        ("track-2", "audio.select_track", json!(1)),
        ("track-3", "subtitles.select_track", json!(1)),
        ("track-0", "subtitles.select_track", Value::Null),
    ] {
        click(&mut cx, "tracks");
        click(&mut cx, row);
        assert_eq!(
            sent(&mut cx),
            [on_block(command, json!({"number": number}))],
            "{row}"
        );
        assert!(!shows(&mut cx, row), "the menu closes");
    }
}

#[gpui::test]
fn a_block_popped_out_or_dropped_on_another_and_the_url_dialog_send_theirs(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-ip.mkv"], false);
    let (first, second) = (block_name(window, 0, &cx), block_name(window, 1, &cx));
    cx.update(|_, cx| api::intercept(cx));

    hover(&mut cx, "block-1");
    click(&mut cx, "pop-out");
    assert_eq!(
        sent(&mut cx),
        [Invocation::new("block.move_to_new_window").on_block(second)]
    );

    drag(&mut cx, "block-0", "block-1");
    assert_eq!(
        sent(&mut cx),
        [Invocation::new("block.move")
            .with_args(json!({"position": 2}))
            .on_block(first)],
        "the place it's dropped on, from 1"
    );

    window
        .update(&mut cx, |workspace, window, cx| {
            workspace.show_open_url(window, cx)
        })
        .unwrap();
    cx.simulate_input(URL);
    press(&mut cx, "enter");
    assert_eq!(
        sent(&mut cx),
        [Invocation::new("app.open")
            .with_args(json!({"urls": [URL]}))
            .in_window(window_name(window, &cx))]
    );
}

#[gpui::test]
fn an_empty_windows_buttons_and_url_form_send_theirs_to_it(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let name = window_name(window, &cx);
    let in_window = |command: &str, args: Value| {
        Invocation::new(command)
            .with_args(args)
            .in_window(name.clone())
    };
    cx.update(|_, cx| api::intercept(cx));

    click(&mut cx, "open-empty");
    assert_eq!(sent(&mut cx), [in_window("window.open_files", Value::Null)]);
    click(&mut cx, "open-url-empty");
    assert_eq!(sent(&mut cx), [in_window("window.open_url", Value::Null)]);

    // The form, as Open URL shows it: what's typed opens in the window.
    window
        .update(&mut cx, |workspace, window, cx| {
            workspace.show_open_url(window, cx)
        })
        .unwrap();
    cx.simulate_input(URL);
    press(&mut cx, "enter");
    assert_eq!(
        sent(&mut cx),
        [in_window("app.open", json!({"urls": [URL]}))]
    );
}
