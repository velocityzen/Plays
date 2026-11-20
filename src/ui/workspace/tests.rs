//! A workspace window driven as someone at the screen would, on gpui's
//! test platform (see `ui::testing`).

mod casting;
mod casting_to_chromecast;
mod casting_to_dlna;
mod sound;

use gpui::{
    AnyWindowHandle, AppContext, Bounds, EntityId, FullscreenStyle, Modifiers, MouseButton, Pixels,
    TestAppContext, VisualTestContext, WindowHandle, point, px, size,
};

use super::place::Place;
use super::{TOOLBAR_SPACE, Workspace};
use crate::app::ShowOnDisplay;
use crate::hotkeys::{ToggleFloatOnTop, ToggleFloatOnTopWhilePlaying};
use crate::settings::{FloatOnTop, FullScreen, PanelLook, Settings};
use crate::ui::pip::PipEvent;
use crate::ui::settings::SettingsView;
use crate::ui::testing::{
    self, OPEN_WAIT, click, double_click, find, fixture, hover, press, shows, wait_until,
};
use crate::ui::theme::backdrop;
use crate::ui::theme::layout::{WINDOW_GAP, WINDOW_MIN_SIZE, WINDOW_SIZE};
use crate::ui::theme::motion::FIT_POLL;
use crate::ui::{blackout, displays};

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
fn a_file_with_chapters_goes_from_one_to_the_next_and_marks_them(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-chapters.mkv"], false);
    let view = window
        .read_with(&cx, |workspace, _| workspace.tiles[0].view.clone())
        .unwrap();
    view.update(&mut cx, |view, _| view.pause());
    let chapters = view.read_with(&cx, |view, _| view.chapters());
    let parts: Vec<_> = chapters
        .iter()
        .map(|chapter| (chapter.start, chapter.title.as_deref()))
        .collect();
    assert_eq!(
        parts,
        [
            (0.0, Some("Opening")),
            (0.8, Some("Middle")),
            (1.4, Some("End"))
        ]
    );
    let position = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.position());

    hover(&mut cx, "tile-0");
    for (key, expected) in [
        ("pagedown", 0.8),
        ("pagedown", 1.4),
        ("pagedown", 1.4),
        ("pageup", 0.8),
        ("pageup", 0.0),
    ] {
        press(&mut cx, key);
        cx.run_until_parked();
        let at = position(&cx);
        assert!(
            (at - expected).abs() < 1e-6,
            "{key} went to {at}, not {expected}"
        );
    }

    // Paused, its controls show, with the chapters after the first marked.
    assert!(shows(&mut cx, "notch-0") && shows(&mut cx, "notch-1"));
    assert!(!shows(&mut cx, "notch-2"));
    cx.update(|_, cx| Settings::update(cx, false, |s| s.chapter_marks = false));
    assert!(!shows(&mut cx, "notch-0"), "unless Settings say not to");
}

/// The backdrops the window draws now: where, and how they look.
fn backdrops(cx: &mut VisualTestContext) -> Vec<(gpui::Bounds<gpui::Pixels>, gpui::BackdropStyle)> {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    cx.update(|window, _| window.painted_backdrops())
}

#[gpui::test]
fn panels_over_the_video_show_it_through_them_as_settings_say(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let view = window
        .read_with(&cx, |workspace, _| workspace.tiles[0].view.clone())
        .unwrap();
    // Paused, its controls show.
    view.update(&mut cx, |view, _| view.pause());
    let seek = find(&mut cx, "seek");

    for look in [
        PanelLook::Frosted,
        PanelLook::Glass,
        PanelLook::Glow,
        PanelLook::Tint,
    ] {
        cx.update(|_, cx| Settings::update(cx, false, |s| s.panels = look));
        let backdrops = backdrops(&mut cx);
        assert!(
            backdrops
                .iter()
                .any(|(bounds, _)| bounds.contains(&seek.center())),
            "{look:?}: the controls show the video through them"
        );
        assert!(
            backdrops
                .iter()
                .all(|(_, style)| Some(*style) == backdrop::for_look(look)),
            "{look:?}: {backdrops:?}"
        );
    }

    cx.update(|_, cx| Settings::update(cx, false, |s| s.panels = PanelLook::Plain));
    assert_eq!(
        backdrops(&mut cx),
        [],
        "plain panels hide what's under them"
    );
    assert!(shows(&mut cx, "seek"), "and still show");
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
fn picture_in_picture_going_back_brings_the_app_and_its_window_forward(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let tile = window
        .read_with(&cx, |workspace, _| workspace.tiles[0].view.clone())
        .unwrap();
    let pip_says = |cx: &mut VisualTestContext, event: PipEvent| {
        cx.update(|window, cx| tile.update(cx, |view, cx| view.on_pip_event(event, window, cx)));
        cx.run_until_parked();
    };
    let before = cx.app_activations();

    // Its button to go back, pressed while another app and Space are in
    // front: the window alone can't come forward from behind them.
    pip_says(&mut cx, PipEvent::Restore);
    assert_eq!(cx.app_activations(), before + 1, "the app comes forward");
    assert!(cx.update(|window, _| window.is_window_active()));

    // One that never showed, or that this tile closed itself, leaves
    // everything where it is, full screen or not.
    cx.update(|window, _| window.toggle_fullscreen());
    pip_says(&mut cx, PipEvent::Stopped);
    assert_eq!(cx.app_activations(), before + 1);
}

/// The display `window` is on.
fn display_of(window: AnyWindowHandle, cx: &mut VisualTestContext) -> Option<u32> {
    window
        .update(cx, |_, window, cx| displays::of(window, cx))
        .unwrap()
}

#[gpui::test]
fn new_windows_open_on_the_display_in_use(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    let main = display_of(first.into(), &mut cx);
    let second = cx.add_display(size(px(2560.), px(1440.)));
    assert_ne!(main, Some(u32::from(second)));

    // A window on the second display, in front.
    let there = cx
        .update(|_, cx| {
            let bounds = Bounds::centered(Some(second), WINDOW_SIZE, cx);
            Workspace::open_window_at(Some(second), bounds, cx)
        })
        .unwrap();
    let mut there_cx = VisualTestContext::from_window(there.into(), &cx);
    there_cx.update(|window, _| window.activate_window());
    there_cx.run_until_parked();

    let next = cx
        .update(|_, cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    assert_eq!(
        display_of(next.into(), &mut cx),
        Some(u32::from(second)),
        "a new window opens where the one in front is"
    );

    cx.update(|_, cx| SettingsView::open_window(cx));
    cx.run_until_parked();
    let settings = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<SettingsView>())
        .expect("a Settings window");
    assert_eq!(
        display_of(settings.into(), &mut cx),
        Some(u32::from(second)),
        "and Settings with it"
    );

    // A tile popped out goes beside its window, on its display.
    open(there, &mut there_cx, &["clip.mp4", "tone.m4a"], false);
    let from = there_cx.update(|window, _| window.bounds());
    let before = there_cx.windows();
    hover(&mut there_cx, "tile-0");
    click(&mut there_cx, "pop-out");
    there_cx.run_until_parked();
    let popped = there_cx
        .windows()
        .into_iter()
        .find(|window| !before.contains(window))
        .expect("a window for the tile");
    assert_eq!(display_of(popped, &mut there_cx), Some(u32::from(second)));
    let bounds = popped
        .update(&mut there_cx, |_, window, _| window.bounds())
        .unwrap();
    assert_eq!(
        bounds.origin,
        point(from.right() + WINDOW_GAP, from.top()),
        "to its right, the tops level"
    );
}

/// A place on display `display`, by its id and name.
fn place_on(display: u32, name: Option<&str>, at: (f32, f32), size: (f32, f32)) -> Place {
    Place {
        display,
        name: name.map(str::to_owned),
        x: at.0,
        y: at.1,
        width: size.0,
        height: size.1,
    }
}

/// Open a window at `place`, in front, and where it went.
fn open_at(place: Option<Place>, cx: &mut VisualTestContext) -> (Option<u32>, Bounds<Pixels>) {
    let window = cx
        .update(|_, cx| Workspace::open_window_where(place, cx))
        .unwrap();
    let mut window_cx = VisualTestContext::from_window(window.into(), cx);
    window_cx.update(|window, _| window.activate_window());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| (displays::of(window, cx), window.bounds()))
}

#[gpui::test]
fn the_first_window_opens_where_the_last_one_closed(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let main = cx.update(|window, cx| displays::of(window, cx));
    let second = u32::from(cx.add_display(size(px(2560.), px(1440.))));
    let bounds = |x: f32, y: f32, width: f32, height: f32| {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    };

    let there = place_on(second, None, (100., 200.), (800., 600.));
    assert_eq!(
        open_at(Some(there), &mut cx),
        (Some(second), bounds(100., 200., 800., 600.)),
        "on its display, at its place and size"
    );

    let renumbered = place_on(second + 7, Some("Display 2"), (100., 200.), (800., 600.));
    assert_eq!(
        open_at(Some(renumbered), &mut cx).0,
        Some(second),
        "a display numbered anew is known by its name"
    );

    let off_the_edge = place_on(second, None, (2400., 1300.), (800., 600.));
    assert_eq!(
        open_at(Some(off_the_edge), &mut cx).1,
        bounds(1760., 840., 800., 600.),
        "on the display, as far as it fits"
    );

    // The window in front is on the second display now; the main one is
    // where the next goes when its own is gone.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let gone = place_on(second + 7, Some("Gone"), (100., 200.), (800., 600.));
    let (display, at) = open_at(Some(gone), &mut cx);
    assert_eq!(display, main, "where a new window opens, when it is gone");
    assert_eq!(at.size, size(px(800.), px(600.)), "at its size");
}

#[gpui::test]
fn the_window_in_front_or_closed_last_is_where_the_next_launch_opens(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    let second = u32::from(cx.add_display(size(px(2560.), px(1440.))));
    let last = |cx: &mut VisualTestContext| cx.update(|_, cx| Workspace::last_place(cx));
    let main = cx.update(|window, cx| displays::of(window, cx)).unwrap();
    assert_eq!(last(&mut cx).map(|place| place.display), Some(main));

    let there = place_on(second, None, (100., 200.), (800., 600.));
    let window = cx
        .update(|_, cx| Workspace::open_window_where(Some(there), cx))
        .unwrap();
    let mut there_cx = VisualTestContext::from_window(window.into(), &cx);
    there_cx.update(|window, _| window.activate_window());
    there_cx.run_until_parked();
    there_cx.simulate_resize(size(px(900.), px(700.)));
    there_cx.run_until_parked();
    assert_eq!(
        last(&mut cx),
        Some(place_on(second, None, (100., 200.), (900., 700.))),
        "the window in front, as it was resized"
    );

    first
        .update(&mut cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(last(&mut cx).map(|place| place.display), Some(main));

    there_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(
        last(&mut cx),
        Some(place_on(
            second,
            Some("Display 2"),
            (100., 200.),
            (900., 700.)
        )),
        "the window closed last, with its display's name"
    );
}

#[gpui::test]
fn leaving_full_screen_on_another_display_goes_back_where_it_was(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let second = u32::from(cx.add_display(size(px(2560.), px(1440.))));
    let away = |cx: &mut VisualTestContext| {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        window
            .read_with(cx, |workspace, _| workspace.is_away())
            .unwrap()
    };
    let here = cx.update(|_, cx| Workspace::last_place(cx));

    cx.dispatch_action(ShowOnDisplay(second));
    assert!(away(&mut cx), "full screen on the other display");
    assert!(cx.update(|window, _| window.is_fullscreen()));

    cx.update(|window, _| window.toggle_fullscreen());
    assert!(!away(&mut cx), "back, out of full screen");
    assert_eq!(cx.update(|_, cx| Workspace::last_place(cx)), here);
}

#[gpui::test]
fn windows_go_full_screen_as_settings_say(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let style = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, _| window.fullscreen_style())
    };
    assert_eq!(style(&mut cx), FullscreenStyle::OwnSpace);

    cx.update(|_, cx| Settings::update(cx, false, |s| s.full_screen = FullScreen::OverDisplay));
    assert_eq!(style(&mut cx), FullscreenStyle::OverDisplay);
    let next = cx
        .update(|_, cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    let next = next
        .update(&mut cx, |_, window, _| window.fullscreen_style())
        .unwrap();
    assert_eq!(next, FullscreenStyle::OverDisplay, "a new window too");
}

#[gpui::test]
fn other_displays_black_out_while_a_window_is_full_screen(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let second = u32::from(cx.add_display(size(px(2560.), px(1440.))));
    // As the app does, outside any window's update.
    let covered = |cx: &mut VisualTestContext| -> Vec<u32> {
        cx.run_until_parked();
        let app: &TestAppContext = cx;
        app.update(|cx| {
            blackout::update(cx);
            blackout::covered(cx)
        })
        .iter()
        .map(|screen| screen.display)
        .collect()
    };
    let full_screen =
        |cx: &mut VisualTestContext| cx.update(|window, _| window.toggle_fullscreen());

    full_screen(&mut cx);
    assert_eq!(covered(&mut cx), [0u32; 0], "not unless Settings say");
    cx.update(|_, cx| Settings::update(cx, false, |s| s.black_out_displays = true));
    assert_eq!(covered(&mut cx), [second], "the other display");

    full_screen(&mut cx);
    assert_eq!(covered(&mut cx), [0u32; 0], "not out of full screen");

    full_screen(&mut cx);
    assert_eq!(covered(&mut cx), [second]);
    cx.deactivate_window();
    assert_eq!(covered(&mut cx), [0u32; 0], "nor with another app in front");
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
    let second = u32::from(cx.add_display(size(px(2560.), px(1440.))));
    cx.dispatch_action(ShowOnDisplay(second));
    assert!(full_screen(&mut cx), "full screen again after the move");
    let (moving, on) = (
        window
            .read_with(&cx, |workspace, _| workspace.moving_to)
            .unwrap(),
        cx.update(|window, cx| displays::of(window, cx)),
    );
    assert_eq!(moving, None, "the move is done");
    assert_eq!(on, Some(second));
}

#[gpui::test]
fn a_display_gone_on_the_way_leaves_the_window_where_it_was(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let here = cx.update(|window, cx| displays::of(window, cx));
    let second = cx.add_display(size(px(2560.), px(1440.)));
    cx.update(|window, _| window.toggle_fullscreen());
    cx.run_until_parked();

    // Picked from a menu that still had it, once it was unplugged: out of
    // full screen here, with nowhere to go on to.
    cx.remove_display(second);
    cx.dispatch_action(ShowOnDisplay(u32::from(second)));
    cx.run_until_parked();

    let (full_screen, on) =
        cx.update(|window, cx| (window.is_fullscreen(), displays::of(window, cx)));
    assert!(!full_screen, "out of full screen");
    assert_eq!(on, here, "on its display");
    let (moving, away) = window
        .read_with(&cx, |workspace, _| {
            (workspace.moving_to, workspace.is_away())
        })
        .unwrap();
    assert_eq!((moving, away), (None, false), "home, the move given up");
}

#[gpui::test]
fn the_cast_menu_puts_the_window_full_screen_on_a_display(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    assert!(
        !shows(&mut cx, "cast"),
        "nothing to pick, with one display and no devices"
    );

    let second = u32::from(cx.add_display(size(px(2560.), px(1440.))));
    click(&mut cx, "cast");
    assert!(shows(&mut cx, "display-0") && shows(&mut cx, "display-1"));
    assert!(
        !shows(&mut cx, "cast-here"),
        "the displays stand for this computer"
    );

    click(&mut cx, "display-1");
    cx.run_until_parked();
    let (full_screen, on) =
        cx.update(|window, cx| (window.is_fullscreen(), displays::of(window, cx)));
    assert!(full_screen, "full screen");
    assert_eq!(on, Some(second), "on the display picked");
    assert!(!shows(&mut cx, "display-0"), "the menu closed");
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
