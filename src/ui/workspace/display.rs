//! Video → Output Device: putting the window full screen on a display, such
//! as an AirPlay one, or playing the tile in front here again while it
//! casts.

use gpui::{Context, Window};

use super::Workspace;
use crate::ui::displays;

impl Workspace {
    /// Put the window full screen on display `id`. One full screen
    /// elsewhere leaves full screen first, as macOS can't move it, and goes
    /// on once it has (`finish_moving`).
    pub fn show_on_display(&mut self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        // While the tile in front casts, picking the display it is on plays
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
        if window.is_fullscreen() {
            window.toggle_fullscreen();
            cx.notify();
        } else {
            self.finish_moving(window);
        }
    }

    /// Move to the display picked and go full screen there, once the window
    /// is out of full screen; and back where it was once out of full screen
    /// there (`come_home`). Runs on every render and tick.
    pub(super) fn finish_moving(&mut self, window: &mut Window) {
        let mut moved = false;
        if !window.is_fullscreen()
            && let Some(id) = self.moving_to.take()
        {
            displays::move_to(window, id);
            window.toggle_fullscreen();
            moved = true;
        }
        self.come_home(window, moved);
    }
}
