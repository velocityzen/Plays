//! Getting something to the workspace window in front.

use gpui::{AnyWindowHandle, App, Context, Window, WindowHandle};

use crate::api::front_window;
use crate::ui::workspace::Workspace;

/// Run `f` on the workspace window in front, which files opened from
/// Finder go to.
///
/// It runs once the current event is handled: gpui hands events to their
/// listeners while it holds the window they came to, and that window can't
/// be updated again until it lets go.
pub(super) fn with_workspace(
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) {
    cx.defer(|cx| {
        if let Some(handle) = front_window(cx) {
            handle
                .update(cx, |workspace, window, cx| f(workspace, window, cx))
                .inspect_err(|err| log::warn!("can't reach the window in front: {err:#}"))
                .ok();
        }
    });
}

/// The first of `windows` that is a workspace window.
pub(super) fn first_workspace(
    windows: impl IntoIterator<Item = AnyWindowHandle>,
) -> Option<WindowHandle<Workspace>> {
    windows.into_iter().find_map(|w| w.downcast::<Workspace>())
}
