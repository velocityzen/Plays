//! Going from block to block, and moving blocks about the grid, with the keys
//! someone at the screen would press: the block they go to is the one
//! shortcuts go to, with a ring round it till the mouse moves.

use gpui::{EntityId, Modifiers, TestAppContext, VisualTestContext, WindowHandle};

use super::{open, players, zoom};
use crate::ui::testing::{self, find, hover, press, shows};
use crate::ui::workspace::Workspace;

const LEFT: &str = "secondary-alt-left";
const RIGHT: &str = "secondary-alt-right";
const UP: &str = "secondary-alt-up";
const DOWN: &str = "secondary-alt-down";
const PREVIOUS: &str = "secondary-[";
const NEXT: &str = "secondary-]";
const MOVE_LEFT: &str = "secondary-alt-shift-left";
const MOVE_RIGHT: &str = "secondary-alt-shift-right";
const MOVE_UP: &str = "secondary-alt-shift-up";
const MOVE_DOWN: &str = "secondary-alt-shift-down";

/// The blocks, in order.
fn blocks(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<EntityId> {
    players(window, cx)
        .iter()
        .map(|&(block, ..)| block)
        .collect()
}

/// Where the block in front is in the window's order.
fn in_front(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> usize {
    window
        .read_with(cx, |workspace, _| {
            let front = workspace.active_view().map(|view| view.entity_id());
            workspace
                .blocks
                .iter()
                .position(|block| Some(block.view.entity_id()) == front)
        })
        .unwrap()
        .expect("a block in front")
}

/// Which block the ring is round, if one is.
fn ringed(cx: &mut VisualTestContext) -> Option<usize> {
    if !shows(cx, "focus-ring") {
        return None;
    }
    let ring = find(cx, "focus-ring");
    let blocks = ["block-0", "block-1", "block-2", "block-3"];
    blocks
        .iter()
        .position(|&block| shows(cx, block) && find(cx, block) == ring)
}

/// Which blocks are muted, in order.
fn muted(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<bool> {
    players(window, cx)
        .iter()
        .map(|&(_, _, muted)| muted)
        .collect()
}

#[gpui::test]
fn keys_go_from_block_to_block_across_the_grid_and_stop_at_its_edges(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    // Two rows of two:
    // 0 1
    // 2 3
    let files = ["clip.mp4", "clip-ip.mkv", "clip.mp4", "tone.m4a"];
    open(window, &mut cx, &files, false);
    assert_eq!(in_front(window, &cx), 0);
    assert_eq!(ringed(&mut cx), None, "no ring till a key is pressed");

    let mut went = Vec::new();
    for keys in [RIGHT, RIGHT, DOWN, DOWN, LEFT, LEFT, UP, UP] {
        press(&mut cx, keys);
        went.push(in_front(window, &cx));
    }
    assert_eq!(went, [1, 1, 3, 3, 2, 2, 0, 0]);

    press(&mut cx, RIGHT);
    press(&mut cx, DOWN);
    assert_eq!(ringed(&mut cx), Some(3), "a ring round the block gone to");
    press(&mut cx, "m");
    assert_eq!(
        muted(window, &cx),
        [false, false, false, true],
        "shortcuts go to it"
    );

    // Closed down to one: no ring round a block alone.
    for _ in 0..3 {
        press(&mut cx, "secondary-w");
    }
    assert_eq!(blocks(window, &cx).len(), 1);
    assert_eq!(ringed(&mut cx), None);
}

#[gpui::test]
fn next_and_previous_go_round_every_block(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(
        window,
        &mut cx,
        &["clip.mp4", "clip-ip.mkv", "tone.m4a"],
        false,
    );

    let mut went = Vec::new();
    for keys in [NEXT, NEXT, NEXT, PREVIOUS, PREVIOUS] {
        press(&mut cx, keys);
        went.push(in_front(window, &cx));
    }
    assert_eq!(went, [1, 2, 0, 2, 1]);
}

#[gpui::test]
fn the_ring_goes_once_the_mouse_moves_or_clicks(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(
        window,
        &mut cx,
        &["clip.mp4", "clip-ip.mkv", "tone.m4a"],
        false,
    );

    // Over the block the keys went to: it stays in front, without a ring.
    press(&mut cx, NEXT);
    assert_eq!(ringed(&mut cx), Some(1));
    hover(&mut cx, "block-1");
    assert_eq!(ringed(&mut cx), None);
    assert_eq!(in_front(window, &cx), 1);

    // Over another: that one's in front.
    press(&mut cx, NEXT);
    assert_eq!(ringed(&mut cx), Some(2));
    hover(&mut cx, "block-0");
    assert_eq!(ringed(&mut cx), None);
    assert_eq!(in_front(window, &cx), 0);

    // Clicked where the mouse rests, without moving it.
    press(&mut cx, NEXT);
    assert_eq!(ringed(&mut cx), Some(1));
    let at = find(&mut cx, "block-0").center();
    cx.simulate_click(at, Modifiers::none());
    assert_eq!(ringed(&mut cx), None);
    assert_eq!(in_front(window, &cx), 0);
}

#[gpui::test]
fn a_block_moves_about_the_grid_staying_in_front(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    // Two rows, the last one short:
    // a b
    // c
    open(
        window,
        &mut cx,
        &["clip.mp4", "clip-ip.mkv", "tone.m4a"],
        false,
    );
    let [a, b, c] = blocks(window, &cx)[..] else {
        panic!("three blocks");
    };
    let playing = |cx: &VisualTestContext| {
        let mut playing: Vec<_> = players(window, cx)
            .iter()
            .map(|&(block, playing, _)| (block, playing))
            .collect();
        playing.sort();
        playing
    };
    let was_playing = playing(&cx);

    press(&mut cx, MOVE_RIGHT);
    assert_eq!(blocks(window, &cx), [b, a, c]);
    press(&mut cx, MOVE_RIGHT);
    assert_eq!(blocks(window, &cx), [b, a, c], "not past the edge");
    press(&mut cx, MOVE_DOWN);
    assert_eq!(blocks(window, &cx), [b, c, a], "to the last block below");
    assert_eq!(in_front(window, &cx), 2);
    assert_eq!(ringed(&mut cx), Some(2));
    press(&mut cx, "m");
    assert_eq!(
        muted(window, &cx),
        [false, false, true],
        "it's still in front"
    );

    press(&mut cx, MOVE_LEFT);
    assert_eq!(blocks(window, &cx), [b, c, a], "nothing to its left");
    press(&mut cx, MOVE_UP);
    assert_eq!(blocks(window, &cx), [a, c, b]);
    press(&mut cx, MOVE_RIGHT);
    press(&mut cx, MOVE_LEFT);
    assert_eq!(blocks(window, &cx), [a, c, b], "and back");
    assert_eq!(in_front(window, &cx), 0);
    assert_eq!(playing(&cx), was_playing, "moving neither plays nor pauses");
}

#[gpui::test]
fn with_a_block_maximized_the_keys_show_another_and_move_none(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(
        window,
        &mut cx,
        &["clip.mp4", "clip-ip.mkv", "tone.m4a"],
        false,
    );
    let [a, b, c] = blocks(window, &cx)[..] else {
        panic!("three blocks");
    };
    let first = window
        .read_with(&cx, |workspace, _| workspace.blocks[0].view.clone())
        .unwrap();
    zoom(&first, &mut cx);
    cx.run_until_parked();
    let maximized = |cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.maximized)
            .unwrap()
    };
    assert_eq!(maximized(&cx), Some(a));

    press(&mut cx, RIGHT);
    assert_eq!(maximized(&cx), Some(b), "the one beside fills the window");
    assert!(shows(&mut cx, "block-1") && !shows(&mut cx, "block-0"));
    assert_eq!(in_front(window, &cx), 1);
    assert_eq!(ringed(&mut cx), None, "no ring round a block alone");

    press(&mut cx, MOVE_LEFT);
    assert_eq!(blocks(window, &cx), [a, b, c], "none moves unseen");
    press(&mut cx, PREVIOUS);
    assert_eq!(maximized(&cx), Some(a));
}
