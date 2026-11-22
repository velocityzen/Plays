//! The command palette over the window: opening it, and doing what it
//! asks once it closes.

use gpui::{AppContext as _, Context, Window};

use super::Workspace;
use crate::api::{self, Target};
use crate::ui::palette::{Entry, Palette, PaletteEvent};
use crate::ui::settings::SettingsView;

impl Workspace {
    /// Show the palette listing `entries`, those run lately (`recents`)
    /// first, what it runs aimed at this window. Focuses the one up
    /// already, if one is.
    pub fn show_palette(
        &mut self,
        entries: Vec<Entry>,
        recents: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((palette, _)) = &self.palette {
            palette.update(cx, |palette, cx| palette.focus(window, cx));
            return;
        }
        let here = Target {
            window: Some(self.id().to_string()),
            tile: None,
        };
        let palette = cx.new(|cx| Palette::new(here, entries, recents, window, cx));
        let subscription = cx.subscribe_in(&palette, window, |this, _, event, window, cx| {
            this.palette = None;
            window.focus(&this.focus_handle);
            cx.notify();
            match event {
                PaletteEvent::Run(invocation) => api::dispatch(invocation.clone(), cx),
                &PaletteEvent::ChangeShortcut(command) => {
                    cx.defer(move |cx| SettingsView::record_shortcut(command, cx));
                }
                PaletteEvent::Dismiss => {}
            }
        });
        self.palette = Some((palette, subscription));
        cx.notify();
    }

    /// The palette, while it's up.
    #[cfg(test)]
    pub fn palette(&self) -> Option<gpui::Entity<Palette>> {
        self.palette.as_ref().map(|(palette, _)| palette.clone())
    }
}
