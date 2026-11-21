//! The empty window's screensaver, on gpui's fake clock: left alone in
//! front, its buttons fade and its logo bounces off its edges; used again,
//! the logo flies back to its place and the buttons come back.

use std::time::Duration;

use gpui::{
    Bounds, Modifiers, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, WindowHandle, point, px,
};

use super::open;
use crate::settings::Settings;
use crate::ui::settings::SettingsView;
use crate::ui::testing::{self, click, find, hover, press, shows};
use crate::ui::theme::screensaver::{RETURN, SPEED};
use crate::ui::workspace::Workspace;

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);

/// Let `time` go by on the fake clock.
fn wait(cx: &mut VisualTestContext, time: Duration) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
}

fn logo(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    find(cx, "logo")
}

fn bouncing(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> bool {
    window
        .read_with(cx, |workspace, _| workspace.screensaver.bouncing())
        .unwrap()
}

fn traffic_lights(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> bool {
    window
        .read_with(cx, |workspace, _| workspace.traffic_lights_shown)
        .unwrap()
}

/// `at` is within a point of `expected`, as laid out.
#[track_caller]
fn near(at: Point<Pixels>, expected: Point<Pixels>) {
    let off = at - expected;
    assert!(
        off.x.abs() <= px(1.) && off.y.abs() <= px(1.),
        "{at:?} isn't {expected:?}"
    );
}

/// A way of using the window, as someone at the screen would.
type Use = fn(&mut VisualTestContext);

/// Change the settings as Settings → Appearance would.
fn set(cx: &mut VisualTestContext, change: impl FnOnce(&mut Settings)) {
    cx.update(|_, cx| Settings::update(cx, false, change));
    cx.run_until_parked();
}

#[gpui::test]
fn left_alone_its_logo_bounces_off_its_edges_and_flies_back_when_used(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let home = logo(&mut cx);
    let room = find(&mut cx, "empty");
    let url_button = find(&mut cx, "open-url-empty");

    wait(&mut cx, 5 * MINUTE - SECOND);
    assert_eq!(logo(&mut cx), home, "not yet");
    assert!(traffic_lights(window, &cx));

    // It sets off across and down, drawn at once, and the window's own
    // buttons go too.
    wait(&mut cx, SECOND);
    assert!(!traffic_lights(window, &cx));
    wait(&mut cx, SECOND);
    near(
        logo(&mut cx).origin,
        home.origin + point(px(1.), px(1.)) * SPEED as f32,
    );

    // It bounces off each edge, and stays within them.
    let (mut across, mut down) = (Vec::new(), Vec::new());
    for _ in 0..120 {
        wait(&mut cx, SECOND / 2);
        let at = logo(&mut cx);
        assert_eq!(at.size, home.size);
        assert!(at.left() >= room.left() - px(1.) && at.right() <= room.right() + px(1.));
        assert!(at.top() >= room.top() - px(1.) && at.bottom() <= room.bottom() + px(1.));
        across.push(at.origin.x);
        down.push(at.origin.y);
    }
    let turns = |line: &[Pixels]| {
        let steps: Vec<bool> = line.windows(2).map(|two| two[1] > two[0]).collect();
        steps.windows(2).filter(|two| two[0] != two[1]).count()
    };
    assert!(turns(&across) >= 2, "{across:?}");
    assert!(turns(&down) >= 2, "{down:?}");

    // The buttons have faded, and a click where one was only wakes it.
    let out = logo(&mut cx);
    cx.simulate_click(url_button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        !shows(&mut cx, "text-input"),
        "a hidden button takes no click"
    );
    assert!(!bouncing(window, &cx));
    near(logo(&mut cx).origin, out.origin);
    assert!(traffic_lights(window, &cx));

    // It flies back quickly, most of the way in half the time.
    wait(&mut cx, RETURN / 2);
    let gone = |at: Point<Pixels>| {
        let off = at - out.origin;
        f32::from(off.x).hypot(f32::from(off.y))
    };
    assert!(gone(logo(&mut cx).origin) > gone(home.origin) * 0.8);
    wait(&mut cx, RETURN / 2);
    assert_eq!(logo(&mut cx), home, "back in its place");
    click(&mut cx, "open-url-empty");
    assert!(shows(&mut cx, "text-input"), "the buttons are back");
}

#[gpui::test]
fn a_click_on_the_logo_draws_it_in_its_next_color_which_it_keeps_through_the_screensaver(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    let home = logo(&mut cx);
    // The color it was last drawn in: a click draws it again itself.
    let drawn = |cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.screensaver.shown_color())
            .unwrap()
    };
    assert_eq!(drawn(&cx), None, "its own color");
    hover(&mut cx, "logo");
    for next in [Some(0), Some(1)] {
        cx.simulate_click(home.center(), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(drawn(&cx), next, "drawn in the next color");
    }
    let moves = window
        .read_with(&cx, |workspace, _| workspace.window_moves)
        .unwrap();
    assert_eq!(moves, 0, "the window stays put");

    // Out for 20 seconds, it bounces off one edge at least and five at
    // most, each a color on. A click on it only brings it back, and it
    // keeps the color it had.
    wait(&mut cx, 5 * MINUTE);
    wait(&mut cx, 20 * SECOND);
    let out = logo(&mut cx);
    let had = drawn(&cx);
    assert_ne!(had, Some(1), "it took others as it bounced");
    cx.simulate_click(out.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(!bouncing(window, &cx));
    wait(&mut cx, RETURN);
    assert_eq!(logo(&mut cx), home);
    assert_eq!(drawn(&cx), had, "it keeps the color it had");
}

#[gpui::test]
fn the_mouse_a_key_or_the_wheel_brings_it_back_and_it_sets_off_again(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let home = logo(&mut cx);
    let uses: [(&str, Use); 3] = [
        ("the mouse", |cx| hover(cx, "empty")),
        ("a key", |cx| press(cx, "a")),
        ("the wheel", |cx| {
            // Where the mouse is, unmoved.
            let position = cx.update(|window, _| window.mouse_position());
            cx.simulate_event(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(-10.))),
                modifiers: Modifiers::none(),
                touch_phase: TouchPhase::Moved,
            });
        }),
    ];
    for (name, used) in uses {
        wait(&mut cx, 5 * MINUTE + SECOND);
        assert!(bouncing(window, &cx), "{name}: it set off again");
        assert_ne!(logo(&mut cx), home);
        used(&mut cx);
        cx.run_until_parked();
        assert!(!bouncing(window, &cx), "{name}");
        wait(&mut cx, RETURN);
        assert_eq!(logo(&mut cx), home, "{name}");
    }

    // Used now and then, it doesn't set off.
    for _ in 0..3 {
        wait(&mut cx, 4 * MINUTE);
        hover(&mut cx, "logo");
    }
    assert!(!bouncing(window, &cx));
}

#[gpui::test]
fn it_waits_as_long_as_the_settings_say_or_not_at_all(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    set(&mut cx, |s| s.screensaver = false);
    wait(&mut cx, 30 * MINUTE);
    assert!(!bouncing(window, &cx), "turned off");

    // Turned on, it counts from then.
    set(&mut cx, |s| {
        s.screensaver = true;
        s.screensaver_minutes = 1;
    });
    wait(&mut cx, MINUTE - SECOND);
    assert!(!bouncing(window, &cx));
    wait(&mut cx, SECOND);
    assert!(bouncing(window, &cx), "after a minute");

    // Turned off while out, it comes back.
    set(&mut cx, |s| s.screensaver = false);
    assert!(!bouncing(window, &cx));
    wait(&mut cx, RETURN);

    // Made shorter than it has rested, it sets off at once.
    set(&mut cx, |s| {
        s.screensaver = true;
        s.screensaver_minutes = 30;
    });
    wait(&mut cx, 10 * MINUTE);
    assert!(!bouncing(window, &cx));
    set(&mut cx, |s| s.screensaver_minutes = 2);
    assert!(bouncing(window, &cx), "rested long enough already");
}

#[gpui::test]
fn it_waits_while_a_url_is_typed_a_file_is_open_or_another_window_is_in_front(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    click(&mut cx, "open-url-empty");
    wait(&mut cx, 30 * MINUTE);
    assert!(!bouncing(window, &cx), "typing a URL");
    press(&mut cx, "escape");
    assert!(!shows(&mut cx, "text-input"));

    // In front again, it counts from then.
    settings_in_front(&mut cx);
    wait(&mut cx, 30 * MINUTE);
    assert!(!bouncing(window, &cx), "behind the Settings window");
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    wait(&mut cx, 5 * MINUTE - SECOND);
    assert!(!bouncing(window, &cx));
    wait(&mut cx, SECOND);
    assert!(bouncing(window, &cx));

    // Sent behind while out, it comes back.
    settings_in_front(&mut cx);
    assert!(!bouncing(window, &cx));
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    // Back in its place, it rests anew.
    wait(&mut cx, RETURN);
    logo(&mut cx);

    open(window, &mut cx, &["clip.mp4"], false);
    wait(&mut cx, 6 * MINUTE);
    assert!(!bouncing(window, &cx), "a file open");
}

/// Open the Settings window in front of the player window.
fn settings_in_front(cx: &mut VisualTestContext) {
    cx.update(|_, cx| SettingsView::open_window(cx));
    cx.run_until_parked();
    let settings = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<SettingsView>())
        .expect("a Settings window");
    settings
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
}
