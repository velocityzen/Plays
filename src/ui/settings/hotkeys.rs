//! Hotkeys: the playback shortcuts, a group for each part of playback.
//! Click + and press keys to add one.

use gpui::prelude::*;
use gpui::{AnyElement, Context, KeyDownEvent, Keystroke, SharedString, Window, div};

use super::form::{group, row, update_settings};
use super::{Pane, SettingsView};
use crate::hotkeys::{self, Command, Group, Hotkeys};
use crate::settings::Settings;
use crate::ui::components::{Button, Icon};
use crate::ui::theme::settings::{
    HOTKEY_ADD_ICON, HOTKEY_CHIP_HEIGHT, HOTKEY_KEYS_MAX_WIDTH, HOTKEY_REMOVE_ICON, HOTKEYS_COLOR,
};
use crate::ui::theme::{Theme, radius};

pub(super) struct HotkeysPane;

/// Waiting for keys to add as a shortcut, and what the last ones did.
#[derive(Default)]
pub(super) struct Recording {
    /// The command whose next keystroke becomes one of its shortcuts.
    command: Option<Command>,
    /// What the last shortcut recorded did to another command.
    note: Option<(Command, SharedString)>,
}

impl Pane for HotkeysPane {
    fn name(&self) -> &'static str {
        "Hotkeys"
    }

    fn icon(&self) -> &'static str {
        "keyboard"
    }

    fn color(&self) -> u32 {
        HOTKEYS_COLOR
    }

    fn content(
        &self,
        view: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let intro = group(
            None,
            theme,
            vec![row(
                "IINA's shortcuts",
                Some(
                    "Shortcuts go to the tile last clicked. Click + and press keys to add one; \
                     menu shortcuts such as ⌘O stay as they are."
                        .into(),
                ),
                theme,
                Button::new("restore-hotkeys", "Restore").on_click(|_, _, cx| {
                    update_settings(cx, true, |s| s.hotkeys = Hotkeys::default())
                }),
            )],
        );
        let mut groups = vec![intro];
        for kind in Group::ALL {
            let rows = Command::ALL
                .into_iter()
                .filter(|command| command.group() == kind)
                .map(|command| {
                    let note = view
                        .recording
                        .note
                        .as_ref()
                        .filter(|(noted, _)| *noted == command)
                        .map(|(_, note)| note.clone());
                    row(
                        command.title(),
                        note,
                        theme,
                        key_chips(view, command, s.hotkeys.keys(command), theme, cx),
                    )
                })
                .collect();
            groups.push(group(Some(kind.title()), theme, rows));
        }
        groups
    }

    /// While waiting for a shortcut, the keys pressed become it. Escape
    /// cancels; the app's own shortcuts (⌘O and so on) never get here.
    fn key_down(
        &self,
        view: &mut SettingsView,
        event: &KeyDownEvent,
        cx: &mut Context<SettingsView>,
    ) {
        let Some(command) = view.recording.command else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let pressed = &event.keystroke;
        if pressed.key == "escape" && !pressed.modifiers.modified() {
            view.recording.command = None;
            cx.notify();
            return;
        }
        let key = Keystroke {
            modifiers: pressed.modifiers,
            key: pressed.key.clone(),
            key_char: None,
        }
        .unparse();
        let mut taken = None;
        update_settings(cx, true, |s| taken = s.hotkeys.add(command, &key));
        view.recording.note = taken.map(|from| {
            let note = format!("{} was for “{}”", hotkeys::display(&key), from.title());
            (command, SharedString::from(note))
        });
        view.recording.command = None;
        cx.notify();
    }

    fn leave(&self, view: &mut SettingsView) {
        view.recording.command = None;
    }
}

/// A command's keys, each with a button to remove it, and a button to add
/// one (or a prompt while waiting for it).
fn key_chips(
    view: &SettingsView,
    command: Command,
    keys: &[String],
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let chip = |id: SharedString| {
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .h(HOTKEY_CHIP_HEIGHT)
            .px_2()
            .superellipse(radius::ITEM)
            .border_1()
            .text_sm()
    };
    let recording = view.recording.command == Some(command);
    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .justify_end()
        .items_center()
        .gap_1()
        .max_w(HOTKEY_KEYS_MAX_WIDTH)
        .children(keys.iter().map(|key| {
            let removed = key.clone();
            chip(format!("{}-{key}", command.id()).into())
                .border_color(theme.border)
                .bg(theme.background)
                .child(hotkeys::display(key))
                .child(
                    div()
                        .id("remove")
                        .flex_none()
                        .cursor_pointer()
                        .superellipse(radius::SMALL)
                        .hover(|style| style.bg(theme.surface_hover))
                        .child(
                            Icon::new("close")
                                .size(HOTKEY_REMOVE_ICON)
                                .color(theme.text_muted),
                        )
                        .on_click(move |_, _, cx| {
                            update_settings(cx, true, |s| s.hotkeys.remove(command, &removed))
                        }),
                )
        }))
        .child(if recording {
            chip(format!("{}-recording", command.id()).into())
                .border_color(theme.accent)
                .text_color(theme.accent)
                .cursor_pointer()
                .child("Press keys… (⎋ cancels)")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.recording.command = None;
                    cx.notify();
                }))
        } else {
            chip(format!("{}-add", command.id()).into())
                .border_color(theme.border)
                .cursor_pointer()
                .hover(|style| style.bg(theme.surface_hover))
                .child(
                    Icon::new("plus")
                        .size(HOTKEY_ADD_ICON)
                        .color(theme.text_muted),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.recording.command = Some(command);
                    this.recording.note = None;
                    window.focus(&this.focus_handle);
                    cx.notify();
                }))
        })
        .into_any_element()
}
