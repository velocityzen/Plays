//! Labels: a window's (`window.rename`), which its title shows, and a
//! tile's (`tile.rename`), which its header shows; what the API names
//! either by besides its id. No two windows have the same one, nor two
//! tiles, in whichever windows.

use gpui::{App, AppContext, Context, EntityId, Window};
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
    pub fn ask_for_label(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.label.clone();
        self.ask(
            "rename-window",
            "Rename Window",
            label.as_deref(),
            window,
            cx,
            |this, label, cx| {
                if this.label_taken(label, cx) {
                    return Err(format!("Another window is called {label}."));
                }
                this.send("window.rename", json!({"label": label}), cx);
                Ok(())
            },
        );
    }

    /// Ask what to call the tile `id`, in a sheet over the window, holding
    /// its label.
    pub fn ask_for_tile_label(
        &mut self,
        id: EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tile) = self.tiles.iter().find(|tile| tile.view.entity_id() == id) else {
            return;
        };
        let label = tile.view.read(cx).label().map(str::to_owned);
        self.ask(
            "rename-tile",
            "Rename Tile",
            label.as_deref(),
            window,
            cx,
            move |this, label, cx| {
                if this.tile_label_taken(label, id, cx) {
                    return Err(format!("Another tile is called {label}."));
                }
                this.send_to_tile(id, "tile.rename", json!({"label": label}), cx);
                Ok(())
            },
        );
    }

    /// Ask for a name in a sheet `id` titled `title`, holding `name`, and
    /// hand the name typed, trimmed, to `name_it`, which says why it won't
    /// do, the sheet staying up. Focuses the sheet up already, if one is.
    fn ask(
        &mut self,
        id: &'static str,
        title: &'static str,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
        name_it: impl Fn(&mut Self, &str, &mut Context<Self>) -> Result<(), String> + 'static,
    ) {
        if let Some((sheet, _)) = &self.naming {
            sheet.update(cx, |sheet, cx| sheet.focus(window, cx));
            return;
        }
        let sheet = cx.new(|cx| NameSheet::new(id, title, name, window, cx));
        let subscription =
            cx.subscribe_in(&sheet, window, move |this, sheet, event, window, cx| {
                if let NameEvent::Name(name) = event
                    && let Err(why) = name_it(this, name.trim(), cx)
                {
                    sheet.update(cx, |sheet, cx| sheet.refuse(why, cx));
                    return;
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

    /// Whether a tile but `id`, here or in another window, is labeled
    /// `label`. This window, being updated, can't be read as the others
    /// are, so its tiles are looked at here.
    fn tile_label_taken(&self, label: &str, id: EntityId, cx: &App) -> bool {
        self.tiles
            .iter()
            .any(|tile| tile.view.entity_id() != id && tile.view.read(cx).label() == Some(label))
            || tile_label_taken(label, id, cx)
    }
}

/// Whether a tile but `id`, in any player window that can be read, is
/// labeled `label`. Blank, none is, as no tile is labeled so.
pub fn tile_label_taken(label: &str, id: EntityId, cx: &App) -> bool {
    player_windows(cx)
        .into_iter()
        .filter_map(|window| window.downcast::<Workspace>()?.read(cx).ok())
        .flat_map(Workspace::players)
        .any(|tile| tile.entity_id() != id && tile.read(cx).label() == Some(label))
}
