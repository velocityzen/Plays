//! A block that fails to draw says why in place of what it plays, as one
//! whose file fails to decode does, rather than closing Plays: a panic
//! while drawing would unwind into the system's drawing callback, which
//! ends the app.

use std::panic::{self, AssertUnwindSafe};

use gpui::prelude::*;
use gpui::{AnyElement, Context, Window, div};

use super::PlayerView;
use crate::media::decoder::panic_message;
use crate::ui::theme::Theme;

impl PlayerView {
    /// The block as it's drawn, unless drawing it panicked: then it stops
    /// and shows why, and only that, until another file opens in it.
    pub(super) fn render_unless_failing(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let opening = self.playback.openings();
        if self.drawing_failed != Some(opening) {
            match panic::catch_unwind(AssertUnwindSafe(|| self.render_block(window, cx))) {
                Ok(block) => return block.into_any_element(),
                Err(panic) => {
                    let why = format!("it couldn't be drawn: {}", panic_message(&*panic));
                    log::error!("{}: {why}", self.title);
                    self.drawing_failed = Some(opening);
                    self.playback.fail(why);
                }
            }
        }
        let theme = Theme::get(window, cx);
        div()
            .id("player")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.block)
            .child(self.render_content(&theme, None, false, cx))
            .into_any_element()
    }
}
