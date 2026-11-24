//! A block closed, its window closed, or moved to a window that then
//! closes, lets its player go: nothing holds its view, and its sound has
//! left the mixer.

use gpui::{TestAppContext, VisualTestContext, WeakEntity, WindowHandle};

use super::open;
use crate::api::{self, Invocation};
use crate::app::AudioEngine;
use crate::ui::player::PlayerView;
use crate::ui::testing;
use crate::ui::workspace::Workspace;

/// The block `at` in `window`, held weakly, as no one else should hold it.
fn block(
    window: WindowHandle<Workspace>,
    at: usize,
    cx: &VisualTestContext,
) -> WeakEntity<PlayerView> {
    window
        .read_with(cx, |workspace, _| workspace.blocks[at].view.downgrade())
        .unwrap()
}

/// Run `command` on the block `at` in `window`, as its header's buttons
/// do. (Every block draws them, so a click can't tell which is meant.)
fn run_on(command: &str, window: WindowHandle<Workspace>, at: usize, cx: &mut VisualTestContext) {
    let name = window
        .read_with(cx, |workspace, cx| {
            workspace.blocks[at].view.read(cx).id().to_string()
        })
        .unwrap();
    let invocation = Invocation::new(command).on_block(name);
    TestAppContext::update(cx, |cx| api::run(invocation, cx)).expect(command);
    cx.run_until_parked();
}

/// How many players the app's output mixes.
fn mixed(cx: &VisualTestContext) -> usize {
    let mixer = TestAppContext::read(cx, AudioEngine::mixer).expect("the app's output");
    mixer.source_count()
}

/// Nothing holds `block` once what's ready has run. Run with
/// `LEAK_BACKTRACE=1` to see where a handle still held was made.
#[track_caller]
fn let_go(block: &WeakEntity<PlayerView>, cx: &mut VisualTestContext) {
    cx.run_until_parked();
    block.assert_released();
}

#[gpui::test]
fn a_closed_block_lets_its_player_go(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], true);
    let (closed, kept) = (block(window, 0, &cx), block(window, 1, &cx));
    assert_eq!(mixed(&cx), 2);

    run_on("block.close", window, 0, &mut cx);
    let_go(&closed, &mut cx);
    assert!(kept.upgrade().is_some(), "the other block plays on");
    assert_eq!(mixed(&cx), 1, "the other block's sound alone");
}

#[gpui::test]
fn a_closed_window_lets_its_players_go(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], true);
    let blocks = [block(window, 0, &cx), block(window, 1, &cx)];
    assert_eq!(mixed(&cx), 2);

    window
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    for block in &blocks {
        let_go(block, &mut cx);
    }
    assert_eq!(mixed(&cx), 0);
}

#[gpui::test]
fn a_block_moved_to_a_window_that_closes_lets_its_player_go(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], true);
    let moved = block(window, 0, &cx);

    let before = cx.windows();
    run_on("block.move_to_new_window", window, 0, &mut cx);
    let popped = cx
        .windows()
        .into_iter()
        .find(|window| !before.contains(window))
        .and_then(|window| window.downcast::<Workspace>())
        .expect("a window for the block");
    assert!(moved.upgrade().is_some(), "the block plays on there");
    assert_eq!(mixed(&cx), 2);

    popped
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    let_go(&moved, &mut cx);
    assert_eq!(mixed(&cx), 1, "the block left behind's sound alone");
}
