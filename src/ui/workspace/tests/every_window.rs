//! The "all" commands, run in one window, reach every block in every
//! window, so a block popped out onto another display stays with the
//! others. Closing all is a window's own, as are Play and Pause Window.

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::Value;

use super::{open, times, wait_for_frames};
use crate::api::{self, Error, Invocation};
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, click, press, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// A window in front with `clip.mp4`, and another behind it with
/// `clip-ip.mkv`, two seconds each at 25 frames a second; both paused,
/// the first at 1.2 seconds and the other at 0.4.
fn two_windows(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Workspace>,
    WindowHandle<Workspace>,
    VisualTestContext,
) {
    let (front, mut visual) = testing::start(cx);
    let back = cx
        .update(|cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    visual.run_until_parked();
    open(front, &mut visual, &["clip.mp4"], false);
    open(back, &mut visual, &["clip-ip.mkv"], false);
    for (window, time) in [(front, 1.2), (back, 0.4)] {
        view(window, &visual).update(&mut visual, |view, _| {
            view.pause();
            view.seek(time);
        });
    }
    visual.update(|window, _| window.activate_window());
    visual.run_until_parked();
    (front, back, visual)
}

/// The player in `window`'s one block.
fn view(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Entity<PlayerView> {
    window
        .read_with(cx, |workspace, _| workspace.blocks[0].view.clone())
        .unwrap()
}

/// Run `invocation` as a deferred command does, outside any window's
/// update.
fn run(cx: &mut VisualTestContext, invocation: Invocation) -> Result<Value, Error> {
    let done = TestAppContext::update(cx, |cx| api::run(invocation, cx));
    cx.run_until_parked();
    done
}

/// Whether the blocks in `windows` play, in order.
fn playing(windows: &[WindowHandle<Workspace>], cx: &VisualTestContext) -> Vec<bool> {
    windows
        .iter()
        .map(|&window| view(window, cx).read_with(cx, |view, _| view.player().is_playing()))
        .collect()
}

#[gpui::test]
fn syncing_and_stepping_all_reach_the_blocks_in_every_window(cx: &mut TestAppContext) {
    let (front, back, mut cx) = two_windows(cx);

    click(&mut cx, "block-0");
    press(&mut cx, "shift-s");
    wait_for_frames(front, &mut cx, 1.2);
    wait_for_frames(back, &mut cx, 1.2);

    press(&mut cx, "alt-shift-right");
    wait_for_frames(front, &mut cx, 1.24);
    wait_for_frames(back, &mut cx, 1.24);
    press(&mut cx, "alt-shift-left");
    wait_for_frames(front, &mut cx, 1.2);
    wait_for_frames(back, &mut cx, 1.2);

    // Synced to a block named in the other window, playing as it does.
    let named = view(back, &cx).read_with(&cx, |view, _| view.id().to_string());
    view(back, &cx).update(&mut cx, |view, _| {
        view.seek(0.2);
        view.play();
    });
    run(
        &mut cx,
        Invocation::new("playback.sync_all").on_block(named),
    )
    .unwrap();
    assert_eq!(playing(&[front, back], &cx), [true, true]);
    let (first, second) = (times(front, &cx)[0].0, times(back, &cx)[0].0);
    assert!(
        first < 1.0 && (first - second).abs() < 0.1,
        "together, where the one named was: {first} and {second}"
    );
}

#[gpui::test]
fn playing_seeking_and_the_sound_of_all_reach_every_window(cx: &mut TestAppContext) {
    let (front, back, mut cx) = two_windows(cx);
    let windows = [front, back];

    // One plays, in the window behind: Play/Pause All pauses it.
    view(back, &cx).update(&mut cx, |view, _| view.play());
    press(&mut cx, "shift-space");
    assert_eq!(playing(&windows, &cx), [false, false]);
    press(&mut cx, "shift-space");
    assert_eq!(playing(&windows, &cx), [true, true]);
    run(&mut cx, Invocation::new("playback.pause_all")).unwrap();
    assert_eq!(playing(&windows, &cx), [false, false]);
    run(&mut cx, Invocation::new("playback.play_all")).unwrap();
    assert_eq!(playing(&windows, &cx), [true, true]);
    run(&mut cx, Invocation::new("playback.pause_all")).unwrap();

    run(&mut cx, Invocation::new("playback.restart_all")).unwrap();
    wait_for_frames(front, &mut cx, 0.0);
    wait_for_frames(back, &mut cx, 0.0);
    press(&mut cx, "shift-right");
    for window in windows {
        let moved = wait_until(&mut cx, |cx| times(window, cx)[0].0 > 0.5);
        assert!(moved, "skipped ahead: {:?}", times(window, &cx));
    }

    let sound = |cx: &VisualTestContext| {
        windows
            .iter()
            .map(|&window| {
                view(window, cx).read_with(cx, |view, _| {
                    let player = view.player();
                    (player.volume(), player.is_muted())
                })
            })
            .collect::<Vec<_>>()
    };
    let before = sound(&cx);
    run(&mut cx, Invocation::new("audio.volume_down_all")).unwrap();
    let after = sound(&cx);
    for (before, after) in before.iter().zip(&after) {
        assert!(
            after.0 < before.0,
            "each turned down: {before:?} to {after:?}"
        );
    }
    run(&mut cx, Invocation::new("audio.mute_all.toggle")).unwrap();
    assert!(sound(&cx).iter().all(|&(_, muted)| muted), "all muted");
}

#[gpui::test]
fn closing_all_is_a_windows_own_and_the_rest_take_no_window(cx: &mut TestAppContext) {
    let (front, back, mut cx) = two_windows(cx);
    let name = front
        .read_with(&cx, |workspace, _| workspace.id().to_string())
        .unwrap();
    let aimed = run(
        &mut cx,
        Invocation::new("playback.play_all").in_window(name.clone()),
    );
    assert!(
        matches!(aimed, Err(Error::BadArgs { .. })),
        "every window's, not one: {aimed:?}"
    );

    run(&mut cx, Invocation::new("block.close_all").in_window(name)).unwrap();
    let blocks = |window: WindowHandle<Workspace>, cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.blocks.len())
            .unwrap()
    };
    assert_eq!((blocks(front, &cx), blocks(back, &cx)), (0, 1));
}

#[gpui::test]
fn play_and_pause_set_a_block_or_a_window_as_asked_whatever_it_was(cx: &mut TestAppContext) {
    let (front, back, mut cx) = two_windows(cx);
    let windows = [front, back];
    let block = view(front, &cx).read_with(&cx, |view, _| view.id().to_string());
    let window = |window: WindowHandle<Workspace>, cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.id().to_string())
            .unwrap()
    };

    // Twice each: the second finds it so already, and leaves it.
    for _ in 0..2 {
        run(
            &mut cx,
            Invocation::new("playback.play").on_block(block.clone()),
        )
        .unwrap();
        assert_eq!(playing(&windows, &cx), [true, false]);
    }
    for _ in 0..2 {
        run(
            &mut cx,
            Invocation::new("playback.pause").on_block(block.clone()),
        )
        .unwrap();
        assert_eq!(playing(&windows, &cx), [false, false]);
    }

    // A window's blocks, not the other's.
    for _ in 0..2 {
        let play = Invocation::new("playback.play_window").in_window(window(back, &cx));
        run(&mut cx, play).unwrap();
        assert_eq!(playing(&windows, &cx), [false, true]);
    }
    run(&mut cx, Invocation::new("playback.play")).unwrap();
    for _ in 0..2 {
        let pause = Invocation::new("playback.pause_window").in_window(window(back, &cx));
        run(&mut cx, pause).unwrap();
        assert_eq!(playing(&windows, &cx), [true, false]);
    }
}
