//! Driving the app's windows in `#[gpui::test]`s, as someone at the
//! screen would: find what's shown by its `debug_selector`, click it,
//! press keys, and wait for files to open. gpui's test platform draws
//! nothing, so tests check what happens and where things are, not pixels.
//! Files decode on threads of their own, outside gpui's test executor, so
//! waiting for them takes real time.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    Bounds, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext,
    VisualTestContext, WindowHandle, point,
};

use crate::app;
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// How long a file in `tests/fixtures` may take to open, or to start
/// playing.
pub const OPEN_WAIT: Duration = Duration::from_secs(10);

/// A file in `tests/fixtures`.
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The app as it starts (default settings, silent audio), and its first
/// window, empty and in front.
pub fn start(cx: &mut TestAppContext) -> (WindowHandle<Workspace>, VisualTestContext) {
    cx.update(app::set_up_for_tests);
    let window = cx
        .update(|cx| Workspace::open_window(WINDOW_SIZE, cx))
        .expect("opening a window");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    // The test platform doesn't bring new windows to the front.
    visual.update(|window, _| window.activate_window());
    visual.run_until_parked();
    (window, visual)
}

/// Run what's ready until `done`, giving decoding threads real time, for
/// at most `OPEN_WAIT`.
pub fn wait_until(
    cx: &mut TestAppContext,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) -> bool {
    let start = Instant::now();
    while start.elapsed() < OPEN_WAIT {
        cx.run_until_parked();
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Draw the window as it is now: what a file says once it opens, say,
/// doesn't draw it again by itself.
fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

/// Where the element tagged `selector` is drawn.
pub fn find(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    draw(cx);
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing on screen is tagged {selector:?}"))
}

/// Whether anything tagged `selector` is drawn.
pub fn shows(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    draw(cx);
    cx.debug_bounds(selector).is_some()
}

/// Move the mouse to the middle of the element tagged `selector`.
pub fn hover(cx: &mut VisualTestContext, selector: &'static str) {
    let at = find(cx, selector).center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
}

/// Click the middle of the element tagged `selector`, the mouse moving
/// there first, as it does.
pub fn click(cx: &mut VisualTestContext, selector: &'static str) {
    hover(cx, selector);
    let at = find(cx, selector).center();
    cx.simulate_click(at, Modifiers::none());
}

/// Double-click the middle of the element tagged `selector`: a click, then
/// one the system counts as the second.
pub fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
    let at = find(cx, selector).center();
    cx.simulate_click(at, Modifiers::none());
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
    });
}

/// Press the middle of the element tagged `selector` and, with the
/// button down, move the mouse by `by`, a few steps at a time as a mouse
/// goes, and let go there.
pub fn drag_by(cx: &mut VisualTestContext, selector: &'static str, by: Point<Pixels>) {
    const STEPS: u8 = 4;
    hover(cx, selector);
    let from = find(cx, selector).center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    let mut at = from;
    for step in 1..=STEPS {
        let share = f32::from(step) / f32::from(STEPS);
        at = point(from.x + by.x * share, from.y + by.y * share);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
}

/// Drag the middle of the element tagged `from` to the middle of the one
/// tagged `to`.
pub fn drag(cx: &mut VisualTestContext, from: &'static str, to: &'static str) {
    let by = find(cx, to).center() - find(cx, from).center();
    drag_by(cx, from, by);
}

/// Press keys, as gpui writes them: "secondary-w" is ⌘W on macOS and
/// Ctrl+W elsewhere; "m", then "space", is "m space".
pub fn press(cx: &mut VisualTestContext, keys: &str) {
    cx.simulate_keystrokes(keys);
}
