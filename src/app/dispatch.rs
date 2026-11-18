//! Getting a command to where it goes: the workspace window in front, and
//! the tile in it that shortcuts go to.

use gpui::{AnyWindowHandle, App, Context, Window, WindowHandle};

use crate::ui::player::PlayerView;
use crate::ui::workspace::Workspace;

/// Run `f` on the workspace window in front, which menus, shortcuts and files
/// opened from Finder go to.
///
/// It runs once the current event is handled: gpui hands actions from menus
/// and keys to their listeners while it holds the window in front, and that
/// window can't be updated again until it lets go.
pub(super) fn with_workspace(
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) {
    cx.defer(|cx| {
        let front = first_workspace(cx.active_window())
            .or_else(|| first_workspace(cx.window_stack().unwrap_or_default()))
            .or_else(|| first_workspace(cx.windows()));
        if let Some(handle) = front {
            handle
                .update(cx, |workspace, window, cx| f(workspace, window, cx))
                .inspect_err(|err| log::warn!("can't reach the window in front: {err:#}"))
                .ok();
        }
    });
}

/// Run `f` on the tile shortcuts go to in the window in front.
pub(super) fn with_active(
    cx: &mut App,
    f: impl FnOnce(&mut PlayerView, &mut Context<PlayerView>) + 'static,
) {
    with_workspace(cx, |workspace, _, cx| workspace.with_active(cx, f));
}

/// The first of `windows` that is a workspace window.
pub(super) fn first_workspace(
    windows: impl IntoIterator<Item = AnyWindowHandle>,
) -> Option<WindowHandle<Workspace>> {
    windows.into_iter().find_map(|w| w.downcast::<Workspace>())
}
