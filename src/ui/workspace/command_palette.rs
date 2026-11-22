//! The command palette, or Go to…, over the window: opening it, and doing
//! what it asks once it closes.

use gpui::{AppContext as _, Context, Window};

use super::Workspace;
use crate::api::{self, Target};
use crate::ui::palette::{Listing, Palette, PaletteEvent};
use crate::ui::settings::SettingsView;

impl Workspace {
    /// Show the palette listing `listing`, what it runs aimed at this
    /// window. Focuses the one up already, if it lists the same, or puts
    /// this one in its place.
    pub fn show_palette(&mut self, listing: Listing, window: &mut Window, cx: &mut Context<Self>) {
        let commands = matches!(listing, Listing::Commands { .. });
        if let Some((palette, _)) = &self.palette
            && palette.read(cx).lists_commands() == commands
        {
            palette.update(cx, |palette, cx| palette.focus(window, cx));
            return;
        }
        let here = Target {
            window: Some(self.id().to_string()),
            tile: None,
        };
        let palette = cx.new(|cx| Palette::new(here, listing, window, cx));
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
