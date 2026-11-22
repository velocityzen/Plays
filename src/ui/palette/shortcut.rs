//! Changing a command's shortcut in the palette: ⌘Return on its row, or
//! Change Shortcut in the footer. Its keys are listed under a heading,
//! each picked to be removed with Return or a click; any other key pressed
//! is added, taken from a command that had it, which a note names. The
//! arrows move among the keys, and Escape goes back to the commands.
//!
//! While it waits for keys, the palette has the focus and its key context
//! has `Recording`, so keys that work anywhere or in a player stay out of
//! the way, as in Settings → Hotkeys, whose saving it shares
//! (`hotkeys::record`).

use gpui::prelude::*;
use gpui::{Context, Hsla, KeyDownEvent, ScrollStrategy, SharedString, Window, div};

use super::{Line, Listing, Palette, Row, RowParts, shown_keys};
use crate::api::Spec;
use crate::hotkeys;
use crate::settings::Settings;

/// The heading over the keys.
const HEADING: &str = "Choose a Key to Remove It";

/// The command whose keys are changing, and what it has.
pub(super) struct Changing {
    pub spec: &'static Spec,
    /// Its keys now, as saved.
    pub keys: Vec<String>,
    /// Whose the key pressed last was, if another command's.
    pub note: Option<SharedString>,
}

impl Palette {
    /// List `spec`'s keys, waiting for one to be pressed.
    pub(super) fn start_changing(
        &mut self,
        spec: &'static Spec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keys = cx.global::<Settings>().hotkeys.keys(spec.id).to_vec();
        self.changing = Some(Changing {
            spec,
            keys,
            note: None,
        });
        self.error = None;
        window.focus(&self.focus_handle);
        self.list_keys(0);
        cx.notify();
    }

    /// The keys, under their heading, the one at `at` picked, or the last.
    fn list_keys(&mut self, at: usize) {
        let count = self
            .changing
            .as_ref()
            .map_or(0, |changing| changing.keys.len());
        self.rows = (0..count)
            .map(|index| Row {
                index,
                at: Vec::new(),
            })
            .collect();
        self.lines = if count == 0 {
            Vec::new()
        } else {
            std::iter::once(Line::Heading(HEADING))
                .chain((0..count).map(Line::Row))
                .collect()
        };
        self.selected = at.min(count.saturating_sub(1));
        self.scroll
            .scroll_to_item(self.line_of(self.selected, true), ScrollStrategy::Center);
    }

    /// Back to the commands, the one whose keys changed picked, each row
    /// with its keys as they are now.
    pub(super) fn stop_changing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(changing) = self.changing.take() else {
            return;
        };
        // Saved by now: what's saved is done before the next key comes.
        let hotkeys = cx.global::<Settings>().hotkeys.clone();
        if let Listing::Commands { entries, .. } = &mut self.listing {
            for entry in entries.iter_mut() {
                entry.keys = shown_keys(hotkeys.keys(entry.spec.id));
            }
        }
        self.focus(window, cx);
        self.filter(cx);
        if let Listing::Commands { entries, .. } = &self.listing
            && let Some(at) = self
                .rows
                .iter()
                .position(|row| entries[row.index].spec.id == changing.spec.id)
        {
            self.selected = at;
            self.scroll
                .scroll_to_item(self.line_of(at, true), ScrollStrategy::Center);
        }
    }

    /// A key pressed while changing: added to the command's keys, and
    /// picked. A key held down adds nothing more.
    pub(super) fn record(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(changing) = &mut self.changing else {
            return;
        };
        // Handled: on macOS a ⌘ key left unhandled goes on to the menu
        // bar, which would run the command whose key it is (⌘Q would
        // quit). Tests have no menu bar to show it.
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let (keys, note) = hotkeys::record(changing.spec.id, &event.keystroke, cx);
        changing.keys = keys;
        changing.note = note.map(SharedString::from);
        // A key added goes last.
        self.list_keys(usize::MAX);
        cx.notify();
    }

    /// Take the key picked from the command's keys.
    pub(super) fn remove_picked(&mut self, cx: &mut Context<Self>) {
        let Some(changing) = &mut self.changing else {
            return;
        };
        let Some(key) = self
            .rows
            .get(self.selected)
            .and_then(|row| changing.keys.get(row.index))
            .cloned()
        else {
            return;
        };
        changing.keys = hotkeys::unrecord(changing.spec.id, &key, cx);
        changing.note = None;
        self.list_keys(self.selected);
        cx.notify();
    }

    /// What a key's row shows: the key, and whether it's one the command
    /// starts out with.
    pub(super) fn key_row(changing: &Changing, index: usize, muted: Hsla) -> RowParts {
        let key = &changing.keys[index];
        let default = hotkeys::is_default(changing.spec.id, key);
        RowParts {
            id: format!("palette-key-{key}").into(),
            check: false,
            label: hotkeys::display(key).into(),
            group: None,
            details: div()
                .flex_none()
                .text_color(muted)
                .when(default, |this| this.child("Default")),
        }
    }
}

#[cfg(test)]
impl Palette {
    /// The command whose keys are changing.
    pub fn changing(&self) -> Option<&'static str> {
        self.changing.as_ref().map(|changing| changing.spec.id)
    }
}

#[cfg(test)]
#[path = "tests/shortcut.rs"]
mod tests;
