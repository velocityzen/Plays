//! Going from window to window from the keyboard: ⌘1 to ⌘9 bring the
//! player window of that number to the front, counted in the order they
//! opened unless one is moved (`window.move`), and Ctrl+Tab and
//! Ctrl+Shift+Tab the one after the window in front or the one before,
//! round and round. ⌘' goes back to the one in front before
//! (`window.recent`), as `window.back` does, which goes further back each
//! time until something else is done.

use gpui::{AnyWindowHandle, App, Global};

use super::history::FocusHistory;
use super::menus;
use crate::api;
use crate::ui::workspace::Workspace;

/// The player windows, in the order ⌘1 to ⌘9 count them: the order they
/// opened, unless one was moved. Closed ones are left out as the list is
/// read.
#[derive(Default)]
struct Opened(Vec<AnyWindowHandle>);
impl Global for Opened {}

/// When each player window was last in front, to go back to.
#[derive(Default)]
struct History(FocusHistory<AnyWindowHandle>);
impl Global for History {}

/// Number the player windows as they open.
pub(super) fn number(cx: &mut App) {
    cx.observe_new::<Workspace>(|_, window, cx| {
        if let Some(window) = window {
            let handle = window.window_handle();
            cx.default_global::<Opened>().0.push(handle);
            // Window → Move Window To has another number.
            menus::refresh(cx);
        }
    })
    .detach();
}

/// `window`, a player window, came to the front.
pub(crate) fn note_front(window: AnyWindowHandle, cx: &mut App) {
    let open = cx.windows();
    let history = &mut cx.default_global::<History>().0;
    history.note(window, |other| !open.contains(&other));
}

/// Bring the player window that was in front last, before the window in
/// front, to the front: twice over, the one in front first.
pub(crate) fn recent(cx: &mut App) {
    let open = players(cx);
    let front = cx.active_window();
    let recent = cx
        .try_global::<History>()
        .and_then(|history| history.0.recent(front, |window| open.contains(&window)));
    if let Some(window) = recent {
        activate(window, cx);
    }
}

/// Bring the player window that was in front before the window in front
/// to the front; run again, with nothing else run between, the one before
/// that, and so on, passing those it has been to.
pub(crate) fn back(cx: &mut App) {
    let open = players(cx);
    let front = cx.active_window();
    let walking = api::ran_before(cx) == Some("window.back");
    let history = &mut cx.default_global::<History>().0;
    if let Some(window) = history.back(front, walking, |window| open.contains(&window)) {
        activate(window, cx);
    }
}

/// Put player window `window` at `place` (from 0) in the numbering, the
/// last place at most; those between move a place to make room.
pub(crate) fn move_to(window: AnyWindowHandle, place: usize, cx: &mut App) {
    let mut players = players(cx);
    let Some(at) = players.iter().position(|&open| open == window) else {
        return;
    };
    players.remove(at);
    players.insert(place.min(players.len()), window);
    cx.default_global::<Opened>().0 = players;
}

/// The player windows open, in the order they opened.
pub(crate) fn players(cx: &App) -> Vec<AnyWindowHandle> {
    let open = cx.windows();
    cx.try_global::<Opened>()
        .map(|opened| {
            opened
                .0
                .iter()
                .copied()
                .filter(|window| open.contains(window))
                .collect()
        })
        .unwrap_or_default()
}

/// Bring the player window `step` from the window in front to the front:
/// 1 for the next, -1 for the one before.
pub(crate) fn step(step: isize, cx: &mut App) {
    let players = players(cx);
    if let Some(window) = stepped(&players, cx.active_window(), step) {
        activate(window, cx);
    }
}

fn activate(window: AnyWindowHandle, cx: &mut App) {
    window
        .update(cx, |_, window, _| window.activate_window())
        .inspect_err(|err| log::warn!("can't bring a window to the front: {err:#}"))
        .ok();
}

/// The window `step` from `front` among `windows`, round and round; from a
/// window not among them (Settings, About), the first going forward and
/// the last going back.
fn stepped<T: Copy + PartialEq>(windows: &[T], front: Option<T>, step: isize) -> Option<T> {
    let count = isize::try_from(windows.len())
        .ok()
        .filter(|&count| count > 0)?;
    let at = match front.and_then(|front| windows.iter().position(|&window| window == front)) {
        Some(at) => (at as isize + step).rem_euclid(count),
        None if step > 0 => 0,
        None => count - 1,
    };
    windows.get(at as usize).copied()
}

#[cfg(test)]
mod tests {
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
}
