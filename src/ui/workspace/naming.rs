//! A window's label (`window.rename`): what its title shows, and what the
//! API names it by besides its id. No two windows have the same one.

use gpui::{App, AppContext, Context, Window};
use serde_json::json;

use super::{TITLE, Workspace};
use crate::app::player_windows;
use crate::ui::name_sheet::{NameEvent, NameSheet};

impl Workspace {
    /// Label the window `label`, trimmed, its title showing it; a blank one
    /// takes its label away. Why not, if another window has it.
    pub fn set_label(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let label = label.trim();
        if self.label_taken(label, cx) {
            return Err(format!("another window is called {label}"));
        }
        self.label = (!label.is_empty()).then(|| label.to_owned());
        window.set_window_title(self.label.as_deref().unwrap_or(TITLE));
        cx.notify();
        Ok(())
    }

    /// Ask what to call the window, in a sheet over it, holding its label.
    /// Focuses it if already up.
    pub fn ask_for_label(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((sheet, _)) = &self.naming {
            sheet.update(cx, |sheet, cx| sheet.focus(window, cx));
            return;
        }
        let label = self.label.clone();
        let sheet = cx.new(|cx| {
            NameSheet::new(
                "rename-window",
                "Rename Window",
                label.as_deref(),
                window,
                cx,
            )
        });
        let subscription = cx.subscribe_in(&sheet, window, |this, sheet, event, window, cx| {
            if let NameEvent::Name(label) = event {
                let label = label.trim();
                if this.label_taken(label, cx) {
                    let why = format!("Another window is called {label}.");
                    sheet.update(cx, |sheet, cx| sheet.refuse(why, cx));
                    return;
                }
                this.send("window.rename", json!({"label": label}), cx);
            }
            this.naming = None;
            window.focus(&this.focus_handle);
            cx.notify();
        });
        self.naming = Some((sheet, subscription));
        cx.notify();
    }

    /// Whether another player window is labeled `label`. Blank, none is.
    /// This one, being updated, can't be read, so it isn't among those
    /// looked at.
    fn label_taken(&self, label: &str, cx: &App) -> bool {
        !label.is_empty()
            && player_windows(cx)
                .into_iter()
                .filter_map(|window| window.downcast::<Self>()?.read(cx).ok())
                .any(|other| other.label.as_deref() == Some(label))
    }
}
