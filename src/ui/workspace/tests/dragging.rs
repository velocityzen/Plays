//! Dragging a tile by its picture, as someone at the screen would: onto
//! another tile, or, with no other tile showing, the window with it.

use gpui::{TestAppContext, VisualTestContext, WindowHandle, point, px};

use super::{open, players};
use crate::ui::testing::{self, double_click, drag, drag_by, fixture, shows, wait_until};
use crate::ui::workspace::Workspace;

/// How many times each tile's picture has moved the window.
fn window_moves(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<usize> {
    window
        .read_with(cx, |workspace, cx| {
            workspace
                .tiles
                .iter()
                .map(|tile| tile.view.read(cx).window_moves)
                .collect()
        })
        .unwrap()
}

#[gpui::test]
fn a_tile_dragged_onto_another_takes_its_place(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(
        window,
        &mut cx,
        &["clip.mp4", "clip-ip.mkv", "clip.mp4"],
        true,
    );
    let tiles = |cx: &VisualTestContext| -> Vec<_> {
        players(window, cx).iter().map(|&(tile, ..)| tile).collect()
    };
    let [a, b, c] = tiles(&cx)[..] else {
        panic!("three tiles");
    };

    drag(&mut cx, "tile-0", "tile-2");
    assert_eq!(tiles(&cx), [b, c, a], "the ones between move over");
    let (active, maximized) = window
        .read_with(&cx, |workspace, _| {
            let active = workspace.active_view().map(|view| view.entity_id());
            (active, workspace.maximized)
        })
        .unwrap();
    assert_eq!(active, Some(a), "the tile dragged is the one in front");
    assert_eq!(maximized, None, "a drag is no double click");
    assert!(
        players(window, &cx).iter().all(|&(_, playing, _)| playing),
        "dragging mustn't pause"
    );

    drag(&mut cx, "tile-2", "tile-1");
    assert_eq!(tiles(&cx), [b, a, c]);

    // Let go over itself, it stays.
    drag_by(&mut cx, "tile-0", point(px(20.), px(20.)));
    assert_eq!(tiles(&cx), [b, a, c]);
    assert_eq!(
        window_moves(window, &cx),
        [0, 0, 0],
        "tiles, not the window"
    );
}

#[gpui::test]
fn with_no_other_tile_showing_the_picture_drags_the_window(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], true);
    let by = point(px(40.), px(30.));

    drag_by(&mut cx, "tile-0", by);
    assert_eq!(window_moves(window, &cx), [1]);
    drag_by(&mut cx, "tile-0", point(px(1.), px(1.)));
    assert_eq!(window_moves(window, &cx), [1], "a press that barely moves");

    // Full screen, the window stays put.
    cx.update(|window, _| window.toggle_fullscreen());
    drag_by(&mut cx, "tile-0", by);
    assert_eq!(window_moves(window, &cx), [1]);
    cx.update(|window, _| window.toggle_fullscreen());

    // Maximized, among others hidden.
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![fixture("clip-ip.mkv")], cx);
        })
        .unwrap();
    let opened = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace.tiles.len() == 2 && workspace.tiles[1].view.read(cx).player().is_known()
            })
            .unwrap()
    });
    assert!(opened, "the second file didn't open");
    double_click(&mut cx, "tile-1");
    assert!(!shows(&mut cx, "tile-0"));
    drag_by(&mut cx, "tile-1", by);
    assert_eq!(window_moves(window, &cx), [1, 1]);
    assert_eq!(players(window, &cx).len(), 2, "no tile moved");
}
