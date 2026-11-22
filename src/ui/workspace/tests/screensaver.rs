//! The empty window's screensaver, on gpui's fake clock: left alone in
//! front, its buttons fade and its logo bounces off its edges; used again,
//! the logo flies back to its place and the buttons come back.

use std::time::Duration;

use gpui::{
    Bounds, Modifiers, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, WindowHandle, point, px,
};

use super::*;
use crate::settings::Settings;
use crate::ui::settings::SettingsView;
use crate::ui::testing::{self, click, find, hover, press, shows};
use crate::ui::theme::screensaver::{RETURN, SPEED};
use crate::ui::workspace::Workspace;
use crate::ui::workspace::tests::open;

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

// The screensaver's own workings, without a window: how the logo goes
// back and forth, when it sets off, and where it flies back to.

#[test]
fn a_line_is_gone_back_and_forth() {
    // Within it, from where it started.
    assert_eq!(fold(2., 3., 10.), (5., 0));
    // To its far end, and back from it.
    assert_eq!(fold(2., 8., 10.), (10., 1));
    assert_eq!(fold(2., 12., 10.), (6., 1));
    // To its near end, and on again.
    assert_eq!(fold(2., 18., 10.), (0., 2));
    assert_eq!(fold(2., 21., 10.), (3., 2));
    assert_eq!(fold(0., 35., 10.), (5., 3));
    // Started past an end, it starts there.
    assert_eq!(fold(15., 1., 10.), (9., 1));
    // No room: it stays put.
    assert_eq!(fold(3., 7., 0.), (0., 0));
    assert_eq!(fold(3., 7., -4.), (0., 0));
}

/// A saver over a room 400 × 300 at (100, 50), the logo's place at
/// (150, 100) in it.
fn saver(now: Instant) -> Screensaver {
    let mut saver = Screensaver::new(now);
    saver.room = Bounds::new(point(px(100.), px(50.)), size(px(400.), px(300.)));
    saver.home = point(px(250.), px(150.));
    saver
}

const LOGO: Pixels = px(100.);

fn seconds(s: f64) -> Duration {
    Duration::from_secs_f64(s)
}

#[test]
fn the_logo_sets_off_once_rested_and_bounces_taking_colors() {
    let start = Instant::now();
    let mut saver = saver(start);
    let after = seconds(60.);
    assert_eq!(saver.rest(after, start + seconds(20.)), Some(seconds(40.)));
    assert_eq!(saver.frame(LOGO, start + seconds(20.)).logo_at, None);
    assert_eq!(saver.rest(after, start + seconds(60.)), None);
    assert!(saver.bouncing());

    let set_off = start + seconds(60.);
    let at = |saver: &mut Screensaver, s: f64| saver.frame(LOGO, set_off + seconds(s));
    let first = at(&mut saver, 0.);
    assert_eq!(first.logo_at, Some(point(px(150.), px(100.))));
    assert_eq!((first.color, first.buttons), (None, 1.), "as it was");

    let fading = at(&mut saver, FADE.as_secs_f64() / 2.);
    assert!((fading.buttons - 0.5).abs() < 1e-3, "{}", fading.buttons);
    let faded = at(&mut saver, FADE.as_secs_f64() * 2.);
    assert!(faded.buttons.abs() < f32::EPSILON);

    // 150 across and 100 down to go: the bottom first, then the right.
    let to_bottom = 100. / SPEED;
    let before = at(&mut saver, to_bottom - 0.1);
    assert_eq!(before.color, None);
    let bottom = at(&mut saver, to_bottom + 0.1);
    assert_eq!(bottom.color, Some(color(0)));
    let right = at(&mut saver, 150. / SPEED + 0.1);
    assert_eq!(right.color, Some(color(1)));
    assert_ne!(color(0), color(1));
    let right_at = right.logo_at.unwrap();
    assert!(
        (f64::from(right_at.x) - (300. - 0.1 * SPEED)).abs() < 0.01,
        "{right_at:?}"
    );
    assert!(
        (f64::from(right_at.y) - (200. - 50. - 0.1 * SPEED)).abs() < 0.01,
        "{right_at:?}"
    );

    // As long as it goes, it stays in the room, and takes each color
    // in turn (two at once in a corner), round again after the last.
    let mut colors = Vec::new();
    for tenth in 0..10_000 {
        let at = at(&mut saver, f64::from(tenth) / 10.).logo_at.unwrap();
        assert!(at.x >= px(0.) && at.x <= px(300.), "{at:?}");
        assert!(at.y >= px(0.) && at.y <= px(200.), "{at:?}");
        if colors.last() != Some(&saver.shown_color) {
            colors.push(saver.shown_color);
        }
    }
    let colors: Vec<usize> = colors.into_iter().flatten().collect();
    assert_eq!(colors[0], 0, "{colors:?}");
    for two in colors.windows(2) {
        let next = (two[0] + 1) % COLORS.len();
        assert!(
            two[1] == next || two[1] == (next + 1) % COLORS.len(),
            "{colors:?}"
        );
    }
    assert!(colors.windows(2).any(|two| two[1] < two[0]), "{colors:?}");
}

#[test]
fn used_the_logo_flies_back_quickly_and_the_buttons_fade_in() {
    let start = Instant::now();
    let mut saver = saver(start);
    assert_eq!(saver.rest(seconds(0.), start), None);
    let out = saver.frame(LOGO, start + seconds(1.)).logo_at.unwrap();
    assert_ne!(out, point(px(150.), px(100.)));

    let woke = start + seconds(2.);
    saver.used(woke);
    assert!(!saver.bouncing());
    // Used again on its way, it goes on.
    saver.used(woke);
    let back = |saver: &mut Screensaver, s: f64| saver.frame(LOGO, woke + seconds(s));
    // From where it was last drawn, in the color it was drawn in: its
    // own, before the first edge, though it would be past it by now.
    let first = back(&mut saver, 0.);
    assert_eq!((first.logo_at, first.buttons), (Some(out), 0.));
    assert_eq!(first.color, None);
    let half = back(&mut saver, RETURN.as_secs_f64() / 2.);
    assert!((half.buttons - 0.5).abs() < 1e-3);
    // Most of the way there in half the time.
    let home = point(px(150.), px(100.));
    let gone = |at: Point<Pixels>| f32::from((at - out).x).hypot(f32::from((at - out).y));
    let whole = gone(home);
    assert!(gone(half.logo_at.unwrap()) > whole * 0.8);
    let there = back(&mut saver, RETURN.as_secs_f64());
    assert_eq!((there.logo_at, there.buttons), (Some(home), 1.));

    saver.settle(woke + RETURN);
    assert_eq!(saver.frame(LOGO, woke + RETURN).logo_at, None);
    assert_eq!(
        saver.rest(seconds(60.), woke + RETURN),
        Some(seconds(60.) - RETURN),
        "resting from when it was used"
    );
}

#[test]
fn a_click_picks_the_next_color_and_it_bounces_on_from_it() {
    let start = Instant::now();
    let mut saver = saver(start);
    assert_eq!(saver.frame(LOGO, start).color, None, "its own");
    let mut picked = Vec::new();
    for _ in 0..=COLORS.len() {
        saver.next_color();
        picked.push(saver.frame(LOGO, start).color);
    }
    let each: Vec<_> = (0..COLORS.len())
        .map(|nth| Some(color(nth)))
        .chain([None])
        .collect();
    assert_eq!(picked, each, "each in turn, then its own again");

    // With the third picked, it sets off in it, and takes the fourth at
    // the bottom edge and the fifth at the right.
    for _ in 0..3 {
        saver.next_color();
    }
    assert_eq!(saver.rest(seconds(0.), start), None);
    let at = |saver: &mut Screensaver, s: f64| saver.frame(LOGO, start + seconds(s)).color;
    assert_eq!(at(&mut saver, 0.), Some(color(2)));
    assert_eq!(at(&mut saver, 100. / SPEED + 0.1), Some(color(3)));
    assert_eq!(at(&mut saver, 150. / SPEED + 0.1), Some(color(4)));
    // Brought back, it keeps the one it was last drawn in, the fifth,
    // and sets off again in it, taking the sixth, then the first.
    saver.used(start + seconds(10.));
    assert_eq!(at(&mut saver, 10.), Some(color(4)));
    saver.settle(start + seconds(10.) + RETURN);
    assert_eq!(at(&mut saver, 11.), Some(color(4)));
    assert_eq!(saver.rest(seconds(0.), start + seconds(11.)), None);
    let again = |saver: &mut Screensaver, s: f64| at(saver, 11. + s);
    assert_eq!(again(&mut saver, 0.), Some(color(4)));
    assert_eq!(again(&mut saver, 100. / SPEED + 0.1), Some(color(5)));
    assert_eq!(again(&mut saver, 150. / SPEED + 0.1), Some(color(0)));
}

#[test]
fn it_rests_only_from_when_it_may() {
    let start = Instant::now();
    let mut saver = saver(start);
    saver.allow(true, start);
    saver.allow(false, start + seconds(10.));
    // Long after, still not allowed: then allowed.
    saver.allow(false, start + seconds(100.));
    saver.allow(true, start + seconds(200.));
    assert_eq!(
        saver.rest(seconds(60.), start + seconds(230.)),
        Some(seconds(30.))
    );

    // Not allowed while out, before it was drawn out: it comes back
    // from its place.
    assert_eq!(saver.rest(seconds(60.), start + seconds(260.)), None);
    saver.allow(false, start + seconds(261.));
    assert!(!saver.bouncing());
    let back = saver.frame(LOGO, start + seconds(261.));
    assert_eq!(back.logo_at, Some(point(px(150.), px(100.))));
}
