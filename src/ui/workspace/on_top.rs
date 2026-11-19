//! Floating player windows above other apps' windows, as Settings say:
//! always, or while something in them plays. Settings and About float along
//! with them, so they don't open underneath.

use gpui::{App, Context, Window};

use super::Workspace;
use crate::settings::{FloatOnTop, Settings};

impl Workspace {
    /// Put the window on top, or back among the others, as Settings and
    /// what plays in it say. Cheap when nothing changes, so it runs on every
    /// render and tick.
    pub(super) fn update_on_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let on_top = match cx.global::<Settings>().float_on_top {
            FloatOnTop::Off => false,
            FloatOnTop::Always => true,
            FloatOnTop::WhilePlaying => self.any_playing(cx),
        };
        if on_top != self.on_top {
            self.on_top = on_top;
            window.set_on_top(on_top);
            cx.defer(float_with_players);
        }
    }
}

/// Keep the windows that aren't players (Settings, About) on top while any
/// player window is.
pub fn float_with_players(cx: &mut App) {
    let windows = cx.windows();
    let on_top = windows
        .iter()
        .filter_map(|handle| handle.downcast::<Workspace>())
        .any(|handle| handle.read(cx).is_ok_and(|workspace| workspace.on_top));
    for handle in windows {
        if handle.downcast::<Workspace>().is_none() {
            handle
                .update(cx, |_, window, _| window.set_on_top(on_top))
                .ok();
        }
    }
}
