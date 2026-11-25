//! Driving the app's windows in `#[gpui::test]`s, as someone at the
//! screen would: find what's shown by its `debug_selector`, click it,
//! press keys, and wait for files to open. gpui's test platform draws
//! nothing, so tests check what happens and where things are, not pixels.
//! Files decode on threads of their own, outside gpui's test executor, so
//! waiting for them takes real time.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::{
    Bounds, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, SharedString, TestAppContext, VisualTestContext, WindowHandle, point, px,
};

use crate::app;
use crate::ui::settings::SettingsView;
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

/// Open the Settings window and bring it to the front.
pub fn open_settings(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.update(|_, cx| SettingsView::open_window(cx));
    cx.run_until_parked();
    let handle = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<SettingsView>())
        .expect("a Settings window");
    let mut settings = VisualTestContext::from_window(handle.into(), cx);
    settings.update(|window, _| window.activate_window());
    settings.run_until_parked();
    settings
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

/// Whether every block in `window` set to play has started, `count` of
/// them: ticking each, as the window does while it doesn't draw, which
/// the test platform does only when asked. One set to play starts once
/// what it plays from is there.
pub fn all_started(window: WindowHandle<Workspace>, count: usize, cx: &mut TestAppContext) -> bool {
    window
        .update(cx, |workspace, _, cx| {
            let players = workspace.players();
            for view in &players {
                view.update(cx, |view, cx| view.background_tick(cx));
            }
            players.len() == count && players.iter().all(|view| view.read(cx).has_started())
        })
        .unwrap()
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

/// Each line of text drawn, as it reads (cut short with "…" where it
/// didn't fit), and where.
pub fn painted(cx: &mut VisualTestContext) -> Vec<(SharedString, Bounds<Pixels>)> {
    draw(cx);
    cx.update(|window, _| window.painted_text())
}

/// Whether the menu row tagged `selector` is checked: drawn with a ✓.
pub fn checked(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    let row = find(cx, selector);
    painted(cx).into_iter().any(|(text, bounds)| {
        let middle = bounds.center().y;
        text == "✓" && row.origin.y <= middle && middle <= row.bottom()
    })
}

/// Scroll what's under the element tagged `selector` with the wheel, a
/// little at a time, until all of it is in the window.
pub fn scroll_to(cx: &mut VisualTestContext, selector: &'static str) {
    let height = cx.update(|window, _| window.viewport_size().height);
    for _ in 0..50 {
        let bounds = find(cx, selector);
        if bounds.bottom() <= height {
            return;
        }
        cx.simulate_event(ScrollWheelEvent {
            position: point(bounds.center().x, height / 2.0),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-40.0))),
            ..ScrollWheelEvent::default()
        });
        cx.run_until_parked();
    }
    panic!("{selector:?} doesn't scroll into the window");
}

/// Scroll what's in the middle of the window back to its top.
pub fn scroll_to_top(cx: &mut VisualTestContext) {
    let size = cx.update(|window, _| window.viewport_size());
    cx.simulate_event(ScrollWheelEvent {
        position: point(size.width / 2.0, size.height / 2.0),
        delta: ScrollDelta::Pixels(point(px(0.0), size.height * 100.0)),
        ..ScrollWheelEvent::default()
    });
    cx.run_until_parked();
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
    let from = find(cx, selector).center();
    drag_from(cx, from, by);
}

/// Press at `from`, the mouse moving there first, and, with the button
/// down, move the mouse by `by`, a few steps at a time as a mouse goes,
/// and let go there.
pub fn drag_from(cx: &mut VisualTestContext, from: Point<Pixels>, by: Point<Pixels>) {
    const STEPS: u8 = 4;
    cx.simulate_mouse_move(from, None, Modifiers::none());
    cx.run_until_parked();
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

/// Keep the errors logged on this thread from now on, for `errors_logged`
/// to return: a test's own, each test running on a thread of its own.
pub fn keep_errors() {
    static KEEPS: OnceLock<bool> = OnceLock::new();
    let keeps = KEEPS.get_or_init(|| {
        let set = log::set_logger(&ErrorKeeper).is_ok();
        log::set_max_level(log::LevelFilter::Error);
        set
    });
    assert!(keeps, "another logger was set");
    KEPT.with_borrow_mut(|kept| *kept = Some(Vec::new()));
}

/// The errors logged on this thread since `keep_errors`.
pub fn errors_logged() -> Vec<String> {
    KEPT.with_borrow(|kept| kept.clone().unwrap_or_default())
}

thread_local! {
    /// The errors logged on this thread, while they're kept.
    static KEPT: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

struct ErrorKeeper;

impl log::Log for ErrorKeeper {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Error
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            KEPT.with_borrow_mut(|kept| {
                if let Some(kept) = kept {
                    kept.push(record.args().to_string());
                }
            });
        }
    }

    fn flush(&self) {}
}
