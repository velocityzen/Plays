//! Hotkeys: every command that takes keys, a group for each namespace.
//! Click + and press keys to add one.

use gpui::prelude::*;
use gpui::{AnyElement, Context, KeyDownEvent, Keystroke, SharedString, Window, div};

use super::form::{group, reset, row, set};
use super::{Pane, SettingsView};
use crate::api::{self, commands::NAMESPACES};
use crate::hotkeys;
use crate::settings::Settings;
use crate::ui::components::{Button, ComponentSize, ControlFrame, Icon, Sizable};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::{HOTKEY_KEYS_MAX_WIDTH, HOTKEYS_COLOR};

pub(super) struct HotkeysPane;

/// Waiting for keys to add as a shortcut, and what the last ones did.
#[derive(Default)]
pub(super) struct Recording {
    /// The command whose next keystroke becomes one of its shortcuts.
    command: Option<&'static str>,
    /// What the last shortcut recorded did to another command.
    note: Option<(&'static str, SharedString)>,
}

impl Recording {
    /// Whether it waits for keys, which keys that work anywhere then leave
    /// to it.
    pub(super) fn is_on(&self) -> bool {
        self.command.is_some()
    }
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
                "Default shortcuts",
                Some(
                    "Shortcuts go to the tile last clicked. Click + and press keys to add one."
                        .into(),
                ),
                theme,
                Button::new("restore-hotkeys", "Restore").on_click(|_, _, cx| reset("hotkeys", cx)),
            )],
        );
        let mut groups = vec![intro];
        for (namespace, title) in NAMESPACES {
            let rows: Vec<AnyElement> = hotkeys::commands()
                .filter(|spec| spec.namespace() == namespace)
                .map(|spec| {
                    let note = view
                        .recording
                        .note
                        .as_ref()
                        .filter(|(noted, _)| *noted == spec.id)
                        .map(|(_, note)| note.clone());
                    row(
                        spec.title,
                        note,
                        theme,
                        key_chips(view, spec.id, s.hotkeys.keys(spec.id), theme, cx),
                    )
                })
                .collect();
            if !rows.is_empty() {
                groups.push(group(Some(title), theme, rows));
            }
        }
        groups
    }

    /// While waiting for a shortcut, the keys pressed become it, those that
    /// work anywhere (⌘O and so on) too, which then stay out of the way
    /// (the key context has `Recording`). Escape cancels.
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
        let mut hotkeys = cx.global::<Settings>().hotkeys.clone();
        let taken = hotkeys.add(command, &key);
        set("hotkeys", &hotkeys, cx);
        view.recording.note = taken.map(|from| {
            let title = api::find(from).map_or(from, |spec| spec.title);
            let note = format!("{} was for “{}”", hotkeys::display(&key), title);
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
    command: &'static str,
    keys: &[String],
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let chip = |id: SharedString| {
        div()
            .id(id)
            .control_frame(ComponentSize::Medium)
            .flex_none()
            .gap_1()
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
            chip(format!("{command}-{key}").into())
                .border_color(theme.border)
                .bg(theme.background)
                // As little room after the remove button as around it.
                .pr_0p5()
                .child(hotkeys::display(key))
                .child(
                    Button::icon("remove", "close")
                        .small()
                        .icon_color(theme.text_muted)
                        .on_click(move |_, _, cx| {
                            let mut hotkeys = cx.global::<Settings>().hotkeys.clone();
                            hotkeys.remove(command, &removed);
                            set("hotkeys", &hotkeys, cx);
                        }),
                )
        }))
        .child(if recording {
            chip(format!("{command}-recording").into())
                .border_color(theme.accent)
                .text_color(theme.accent)
                .cursor_pointer()
                .child("Press keys… (⎋ cancels)")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.recording.command = None;
                    cx.notify();
                }))
        } else {
            chip(format!("{command}-add").into())
                .debug_selector(move || format!("hotkey-add-{command}"))
                .border_color(theme.border)
                .cursor_pointer()
                .hover(|style| style.bg(theme.surface_hover))
                .child(Icon::new("plus").small().color(theme.text_muted))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.recording.command = Some(command);
                    this.recording.note = None;
                    window.focus(&this.focus_handle);
                    cx.notify();
                }))
        })
        .into_any_element()
}
