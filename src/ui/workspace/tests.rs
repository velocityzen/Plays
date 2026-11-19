//! A workspace window driven as someone at the screen would, on gpui's
//! test platform (see `ui::testing`).

mod casting;
mod casting_to_chromecast;
mod casting_to_dlna;

use gpui::{
    AppContext, EntityId, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle,
    px, size,
};

use super::{TOOLBAR_SPACE, Workspace};
use crate::app::ShowOnDisplay;
use crate::hotkeys::{ToggleFloatOnTop, ToggleFloatOnTopWhilePlaying};
use crate::settings::{FloatOnTop, Settings};
use crate::ui::displays;
use crate::ui::settings::SettingsView;
use crate::ui::testing::{
    self, OPEN_WAIT, click, double_click, find, fixture, hover, press, shows, wait_until,
};
use crate::ui::theme::layout::WINDOW_MIN_SIZE;
use crate::ui::theme::motion::FIT_POLL;

/// Open `files` into `window`, and wait until every one of them has
/// opened, and, with `playing`, is playing.
fn open(
    window: WindowHandle<Workspace>,
    cx: &mut VisualTestContext,
    files: &[&str],
    playing: bool,
) {
    let paths = files.iter().map(|file| fixture(file)).collect();
    window
        .update(cx, |workspace, _, cx| workspace.open_paths(paths, cx))
        .unwrap();
    let ready = wait_until(cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace.tiles.len() == files.len()
                    && workspace.tiles.iter().all(|tile| {
                        let player = tile.view.read(cx).player();
                        player.info().is_some() && (!playing || player.is_playing())
                    })
            })
            .unwrap()
    });
    assert!(ready, "{files:?} didn't open (and play) in {OPEN_WAIT:?}");
}

/// The tiles' players, in order: (tile, playing, muted).
fn players(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<(EntityId, bool, bool)> {
    window
        .read_with(cx, |workspace, cx| {
            workspace
                .tiles
                .iter()
                .map(|tile| {
                    let player = tile.view.read(cx).player();
                    (
                        tile.view.entity_id(),
                        player.is_playing(),
                        player.is_muted(),
                    )
                })
                .collect()
        })
        .unwrap()
}

#[gpui::test]
fn opens_a_file_in_a_tile_and_fits_the_window_to_it(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let info = window
        .read_with(&cx, |workspace, cx| {
            workspace.tiles[0].view.read(cx).player().info()
        })
        .unwrap()
        .unwrap();
    assert_eq!(info.video_size, Some((160, 120)));
    assert!(info.has_audio);

    // The window waits for the video's size, then fits it: at its
    // smallest here, as the video is smaller still.
    cx.executor().advance_clock(FIT_POLL);
    cx.run_until_parked();
    let fitted = cx.update(|window, _| window.bounds().size);
    assert_eq!(
        fitted,
        size(
            px(160.).max(WINDOW_MIN_SIZE.width),
            (px(120.) + TOOLBAR_SPACE).max(WINDOW_MIN_SIZE.height)
        )
    );
}

#[gpui::test]
fn a_click_picks_a_tile_without_pausing_it(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-ip.mkv"], true);

    click(&mut cx, "tile-1");
    let [_, (second, playing, _)] = players(window, &cx)[..] else {
        panic!("two tiles");
    };
    assert!(playing, "a single click mustn't pause the tile");
    let active = window
        .read_with(&cx, |workspace, _| workspace.active)
        .unwrap();
    assert_eq!(
        active,
        Some(second),
        "the tile clicked is the one shortcuts go to"
    );
}

#[gpui::test]
fn a_double_click_fills_the_window_with_a_tile_and_another_puts_it_back(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-ip.mkv"], true);
    let viewport = cx.update(|window, _| window.viewport_size());
    let content = size(viewport.width, viewport.height - TOOLBAR_SPACE);

    double_click(&mut cx, "tile-1");
    assert!(!shows(&mut cx, "tile-0"), "the other tile is hidden");
    assert_eq!(find(&mut cx, "tile-1").size, content);
    assert!(
        players(window, &cx).iter().all(|&(_, playing, _)| playing),
        "double-clicking mustn't pause"
    );

    double_click(&mut cx, "tile-1");
    assert!(shows(&mut cx, "tile-0"));
    assert!(find(&mut cx, "tile-1").size.width < content.width / 2.);
}

#[gpui::test]
fn shortcuts_go_to_the_tile_last_clicked(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-ip.mkv"], false);
    let muted = |cx: &VisualTestContext| {
        players(window, cx)
            .iter()
            .map(|&(_, _, muted)| muted)
            .collect::<Vec<_>>()
    };

    click(&mut cx, "tile-1");
    press(&mut cx, "m");
    assert_eq!(muted(&cx), [false, true]);

    click(&mut cx, "tile-0");
    press(&mut cx, "m");
    assert_eq!(muted(&cx), [true, true]);
}

/// The tiles' players, in order: (where each is, the frame it shows,
/// playing).
fn times(window: WindowHandle<Workspace>, cx: &impl AppContext) -> Vec<(f64, Option<f64>, bool)> {
    window
        .read_with(cx, |workspace, cx| {
            workspace
                .tiles
                .iter()
                .map(|tile| {
                    let player = tile.view.read(cx).player();
                    (player.position(), player.frame_pts(), player.is_playing())
                })
                .collect()
        })
        .unwrap()
}

/// Wait until every tile shows the frame at `pts`, paused.
fn wait_for_frames(window: WindowHandle<Workspace>, cx: &mut VisualTestContext, pts: f64) {
    let shown = wait_until(cx, |cx| {
        // Frames show as the window draws.
        window.update(cx, |_, window, _| window.refresh()).unwrap();
        times(window, cx).iter().all(|&(_, frame, playing)| {
            !playing && frame.is_some_and(|frame| (frame - pts).abs() < 1e-3)
        })
    });
    assert!(shown, "every tile at {pts}: {:?}", times(window, cx));
}

#[gpui::test]
fn sync_all_puts_every_tile_where_the_one_in_front_is_and_steps_them_together(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    // Two seconds at 25 frames a second each.
    open(window, &mut cx, &["clip.mp4", "clip-ip.mkv"], false);
    let tile = |index: usize, cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.tiles[index].view.clone())
            .unwrap()
    };
    for (index, time) in [(0, 1.2), (1, 0.4)] {
        tile(index, &cx).update(&mut cx, |view, _| {
            view.pause();
            view.seek(time);
        });
    }

    click(&mut cx, "tile-0");
    press(&mut cx, "shift-s");
    wait_for_frames(window, &mut cx, 1.2);

    press(&mut cx, "alt-shift-right");
    wait_for_frames(window, &mut cx, 1.24);
    for pts in [1.2, 1.16] {
        press(&mut cx, "alt-shift-left");
        wait_for_frames(window, &mut cx, pts);
    }

    // The one in front playing: the others go back to it, and play.
    tile(1, &cx).update(&mut cx, |view, _| {
        view.seek(0.2);
        view.play();
    });
    click(&mut cx, "tile-1");
    press(&mut cx, "shift-s");
    let [(first, _, first_playing), (second, _, second_playing)] = times(window, &cx)[..] else {
        panic!("two tiles");
    };
    assert!(first_playing && second_playing, "both play, as it does");
    assert!(
        first < 1.0 && (first - second).abs() < 0.1,
        "together, where it was: {first} and {second}"
    );
}

#[gpui::test]
fn shortcuts_go_to_the_tile_the_mouse_is_over(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-ip.mkv"], false);
    let muted = |cx: &VisualTestContext| {
        players(window, cx)
            .iter()
            .map(|&(_, _, muted)| muted)
            .collect::<Vec<_>>()
    };

    hover(&mut cx, "tile-1");
    press(&mut cx, "m");
    assert_eq!(muted(&cx), [false, true]);

    hover(&mut cx, "tile-0");
    press(&mut cx, "m");
    assert_eq!(muted(&cx), [true, true]);

    // Dragging, as a slider of the tile in front is, doesn't move them.
    let over = find(&mut cx, "tile-1").center();
    cx.simulate_mouse_move(over, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    press(&mut cx, "m");
    assert_eq!(muted(&cx), [false, true], "still the tile it came from");
}

#[gpui::test]
fn shift_up_and_down_change_every_tiles_volume_from_its_own(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-ip.mkv"], false);
    let volumes = |cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, cx| {
                workspace
                    .tiles
                    .iter()
                    .map(|tile| (tile.view.read(cx).player().volume() * 100.0).round())
                    .collect::<Vec<_>>()
            })
            .unwrap()
    };
    let quieter = window
        .read_with(&cx, |workspace, _| workspace.tiles[1].view.clone())
        .unwrap();
    quieter.update(&mut cx, |view, _| view.set_volume(0.5));

    hover(&mut cx, "tile-0");
    press(&mut cx, "shift-up");
    assert_eq!(volumes(&cx), [85.0, 55.0]);
    press(&mut cx, "shift-down shift-down");
    assert_eq!(volumes(&cx), [75.0, 45.0]);
}

#[gpui::test]
fn command_w_closes_the_tile_in_front_then_the_window(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-ip.mkv"], false);
    let second = players(window, &cx)[1].0;

    click(&mut cx, "tile-0");
    press(&mut cx, "secondary-w");
    let left: Vec<_> = players(window, &cx).iter().map(|&(id, ..)| id).collect();
    assert_eq!(left, [second], "only the tile in front closes");

    press(&mut cx, "secondary-w");
    assert!(players(window, &cx).is_empty());
    assert!(
        cx.windows().contains(&window.into()),
        "the window stays, empty"
    );

    press(&mut cx, "secondary-w");
    assert!(
        !cx.windows().contains(&window.into()),
        "an empty window closes"
    );
}

#[gpui::test]
fn tiles_sit_in_the_grid(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    // Four tiles make two rows of two.
    open(
        window,
        &mut cx,
        &["clip.mp4", "clip-ip.mkv", "clip.mp4", "tone.m4a"],
        false,
    );
    let [a, b, c, d] = ["tile-0", "tile-1", "tile-2", "tile-3"].map(|tile| find(&mut cx, tile));

    assert_eq!((a.top(), c.top()), (b.top(), d.top()), "rows line up");
    assert_eq!(
        (a.left(), b.left()),
        (c.left(), d.left()),
        "columns line up"
    );
    assert!(b.left() > a.right() && c.top() > a.bottom());
    assert!(
        [b, c, d].iter().all(|tile| tile.size == a.size),
        "every tile gets the same room"
    );
}

/// Whether the player window, and the Settings window, float above other
/// apps' windows.
fn on_top(cx: &mut VisualTestContext) -> (bool, bool) {
    cx.run_until_parked();
    let player = cx.update(|window, _| window.is_on_top());
    let settings = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<SettingsView>())
        .expect("a Settings window");
    let settings = settings
        .update(cx, |_, window, _| window.is_on_top())
        .unwrap();
    (player, settings)
}

#[gpui::test]
fn floats_on_top_always_or_while_playing(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], true);
    cx.update(|_, cx| SettingsView::open_window(cx));
    assert_eq!(on_top(&mut cx), (false, false), "not unless asked to");

    // From the Video menu, with Settings floating along so they don't
    // open under the player.
    cx.dispatch_action(ToggleFloatOnTopWhilePlaying);
    assert_eq!(
        cx.update(|_, cx| cx.global::<Settings>().float_on_top),
        FloatOnTop::WhilePlaying
    );
    assert_eq!(on_top(&mut cx), (true, true), "playing");

    click(&mut cx, "tile-0");
    press(&mut cx, "space");
    let paused = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                !workspace.tiles[0].view.read(cx).player().is_playing()
            })
            .unwrap()
    });
    assert!(paused);
    assert_eq!(on_top(&mut cx), (false, false), "paused");

    cx.dispatch_action(ToggleFloatOnTop);
    assert_eq!(on_top(&mut cx), (true, true), "always, though paused");

    cx.dispatch_action(ToggleFloatOnTop);
    assert_eq!(
        cx.update(|_, cx| cx.global::<Settings>().float_on_top),
        FloatOnTop::Off,
        "the same item again turns it off"
    );
    assert_eq!(on_top(&mut cx), (false, false));
}

#[gpui::test]
fn output_device_shows_the_window_full_screen_on_a_display(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let here = cx
        .update(|window, cx| displays::of(window, cx))
        .expect("a display");
    let full_screen = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, _| window.is_fullscreen())
    };

    cx.dispatch_action(ShowOnDisplay(here));
    assert!(full_screen(&mut cx), "full screen on the display it is on");
    cx.dispatch_action(ShowOnDisplay(here));
    assert!(full_screen(&mut cx), "and still, picked again");

    // Another display: out of full screen here, then in again there.
    cx.dispatch_action(ShowOnDisplay(here + 1));
    assert!(full_screen(&mut cx), "full screen again after the move");
    let moving = window
        .read_with(&cx, |workspace, _| workspace.moving_to)
        .unwrap();
    assert_eq!(moving, None, "the move is done");
}

#[gpui::test]
fn a_tile_alone_expands_to_full_screen_while_it_floats(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], true);
    cx.dispatch_action(ToggleFloatOnTopWhilePlaying);
    let full_screen = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, _| window.is_fullscreen())
    };
    assert!(cx.update(|window, _| window.is_on_top()), "floating");

    double_click(&mut cx, "tile-0");
    assert!(full_screen(&mut cx), "a double click goes full screen");
    double_click(&mut cx, "tile-0");
    assert!(!full_screen(&mut cx), "and another comes back");

    click(&mut cx, "maximize");
    assert!(full_screen(&mut cx), "so does the button");
    click(&mut cx, "maximize");
    assert!(!full_screen(&mut cx));
}
