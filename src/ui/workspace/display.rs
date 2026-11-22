//! Output Device: putting the window full screen on a display, such as an
//! AirPlay one, or playing the block in front here again while it casts.

use gpui::{Context, Window};

use super::Workspace;
use crate::ui::displays;

impl Workspace {
    /// Put the window full screen on display `id`. One full screen
    /// elsewhere leaves full screen first, as a window full screen can't
    /// move, and goes on once it has (`finish_moving`).
    pub fn show_on_display(&mut self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        // While the block in front casts, picking the display it is on plays
        // it here again, as it was.
        if self.end_active_cast(cx) && displays::of(window, cx) == Some(id) {
            return;
        }
        if displays::of(window, cx) == Some(id) {
            self.moving_to = None;
            if !window.is_fullscreen() {
                window.toggle_fullscreen();
            }
            return;
        }
        self.leave_home();
        self.moving_to = Some(id);
        self.follow_display(id, cx);
        if window.is_fullscreen() {
            window.toggle_fullscreen();
            cx.notify();
        } else {
            self.finish_moving(window, cx);
        }
    }

    /// Move to the display picked and go full screen there, once the window
    /// is out of full screen; and back where it was once out of full screen
    /// there (`come_home`). A display that went away on the way leaves the
    /// window out of full screen, back where it was. Runs on every render
    /// and tick.
    pub(super) fn finish_moving(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut moved = false;
        if !window.is_fullscreen()
            && let Some(id) = self.moving_to.take()
        {
            if displays::find(id, cx).is_some() {
                displays::full_screen_on(window, id, cx);
                moved = true;
            } else if let Some(home) = &mut self.home {
                home.shown = true;
            }
        }
        self.come_home(window, moved, cx);
    }
}
