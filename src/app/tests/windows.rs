use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowId};

use super::*;
use crate::ui::settings::SettingsView;
use crate::ui::testing::{self, press};
use crate::ui::theme::layout::WINDOW_SIZE;

/// Open another player window, behind the one in front, as the test
/// platform leaves new windows.
fn open_player(cx: &mut VisualTestContext) -> AnyWindowHandle {
    let window = cx
        .update(|_, cx| Workspace::open_window(WINDOW_SIZE, cx))
        .expect("opening a window");
    cx.run_until_parked();
    window.into()
}

fn in_front(cx: &mut VisualTestContext) -> Option<WindowId> {
    cx.update(|_, cx| cx.active_window().map(|window| window.window_id()))
}

/// Press `keys` in the window in front.
fn press_in_front(cx: &mut VisualTestContext, keys: &str) {
    let front = cx
        .update(|_, cx| cx.active_window())
        .expect("a window in front");
    let mut window = VisualTestContext::from_window(front, cx);
    press(&mut window, keys);
    window.run_until_parked();
}

fn open_settings(cx: &mut VisualTestContext) -> AnyWindowHandle {
    cx.update(|_, cx| SettingsView::open_window(cx));
    cx.run_until_parked();
    let settings = cx
        .windows()
        .into_iter()
        .find(|window| window.downcast::<SettingsView>().is_some())
        .expect("a Settings window");
    let mut window = VisualTestContext::from_window(settings, cx);
    window.update(|window, _| window.activate_window());
    window.run_until_parked();
    settings
}

#[gpui::test]
fn the_number_keys_and_ctrl_tab_bring_player_windows_to_the_front(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    let first: AnyWindowHandle = first.into();
    let second = open_player(&mut cx);
    let third = open_player(&mut cx);
    assert_eq!(in_front(&mut cx), Some(first.window_id()));

    press_in_front(&mut cx, "secondary-3");
    assert_eq!(in_front(&mut cx), Some(third.window_id()), "⌘3");
    press_in_front(&mut cx, "secondary-2");
    assert_eq!(in_front(&mut cx), Some(second.window_id()), "⌘2");
    press_in_front(&mut cx, "secondary-9");
    assert_eq!(
        in_front(&mut cx),
        Some(second.window_id()),
        "⌘9: there's no ninth"
    );

    press_in_front(&mut cx, "ctrl-tab");
    assert_eq!(in_front(&mut cx), Some(third.window_id()), "the next");
    press_in_front(&mut cx, "ctrl-tab");
    assert_eq!(
        in_front(&mut cx),
        Some(first.window_id()),
        "round to the first"
    );
    press_in_front(&mut cx, "ctrl-shift-tab");
    assert_eq!(
        in_front(&mut cx),
        Some(third.window_id()),
        "back round to the last"
    );
    press_in_front(&mut cx, "ctrl-shift-tab");
    assert_eq!(
        in_front(&mut cx),
        Some(second.window_id()),
        "the one before"
    );

    // From Settings, which isn't counted: the first, the last, or by
    // number.
    open_settings(&mut cx);
    press_in_front(&mut cx, "ctrl-tab");
    assert_eq!(
        in_front(&mut cx),
        Some(first.window_id()),
        "from Settings, the first"
    );
    open_settings(&mut cx);
    press_in_front(&mut cx, "ctrl-shift-tab");
    assert_eq!(
        in_front(&mut cx),
        Some(third.window_id()),
        "from Settings, the last"
    );
    open_settings(&mut cx);
    press_in_front(&mut cx, "secondary-2");
    assert_eq!(
        in_front(&mut cx),
        Some(second.window_id()),
        "from Settings, ⌘2"
    );

    // A window closed, the ones after it move up a number, and one
    // opened takes the next.
    let mut window = VisualTestContext::from_window(second, &cx);
    window.update(|window, _| window.remove_window());
    window.run_until_parked();
    let mut window = VisualTestContext::from_window(first, &cx);
    window.update(|window, _| window.activate_window());
    window.run_until_parked();
    press_in_front(&mut cx, "secondary-2");
    assert_eq!(
        in_front(&mut cx),
        Some(third.window_id()),
        "the third is second now"
    );
    let fourth = open_player(&mut cx);
    press_in_front(&mut cx, "secondary-3");
    assert_eq!(
        in_front(&mut cx),
        Some(fourth.window_id()),
        "and one opened third"
    );
    press_in_front(&mut cx, "ctrl-tab");
    assert_eq!(
        in_front(&mut cx),
        Some(first.window_id()),
        "round past the closed one"
    );
}

#[test]
fn steps_round_and_round_from_the_window_in_front() {
    let windows = [10, 20, 30];
    assert_eq!(stepped(&windows, Some(10), 1), Some(20));
    assert_eq!(stepped(&windows, Some(30), 1), Some(10), "round");
    assert_eq!(stepped(&windows, Some(20), -1), Some(10));
    assert_eq!(stepped(&windows, Some(10), -1), Some(30), "round back");
    assert_eq!(
        stepped(&windows, Some(99), 1),
        Some(10),
        "from Settings, the first"
    );
    assert_eq!(stepped(&windows, None, -1), Some(30), "or the last");
    assert_eq!(stepped::<i32>(&[], Some(10), 1), None);
    assert_eq!(stepped(&[10], Some(10), 1), Some(10), "the only one stays");
}
