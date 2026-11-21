//! The keymap: the app's own shortcuts and the playback ones from Settings,
//! bound again whenever Settings change them.

use gpui::{App, Global, KeyBinding};

use super::{
    CloseAll, CloseFile, CloseWindow, GoToWindow, NextWindow, OpenFiles, OpenSettings, OpenUrl,
    PreviousWindow, Quit, menus,
};
use crate::hotkeys::Hotkeys;
use crate::settings::Settings;
use crate::ui::{open_url, pairing, text_input};

/// The shortcuts in the keymap, to tell when Settings change them.
struct BoundHotkeys(Hotkeys);
impl Global for BoundHotkeys {}

/// Put the app's shortcuts and the playback ones from Settings in the
/// keymap, and show them in the menus.
pub(super) fn bind_keys(cx: &mut App) {
    let hotkeys = cx.global::<Settings>().hotkeys.clone();
    cx.clear_key_bindings();
    text_input::bind_keys(cx);
    open_url::bind_keys(cx);
    pairing::bind_keys(cx);
    cx.bind_keys(app_bindings());
    cx.bind_keys(hotkeys.bindings());
    cx.set_global(BoundHotkeys(hotkeys));
    menus::set_menus(cx);
}

/// The app's own shortcuts, which Settings don't change.
fn app_bindings() -> Vec<KeyBinding> {
    let mut bindings = vec![
        KeyBinding::new("secondary-o", OpenFiles, None),
        KeyBinding::new("secondary-shift-o", OpenUrl, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-q", Quit, None),
        // The menu shows the first of an action's keys: ⌥⌘W for the window.
        // ⌘W closes the file in front, or the window when it shows none
        // (an empty one, Settings).
        KeyBinding::new("secondary-alt-w", CloseWindow, None),
        KeyBinding::new("secondary-w", CloseWindow, None),
        KeyBinding::new("secondary-w", CloseFile, Some("Workspace")),
        KeyBinding::new("secondary-shift-w", CloseAll, Some("Workspace")),
        KeyBinding::new("ctrl-tab", NextWindow, None),
        KeyBinding::new("ctrl-shift-tab", PreviousWindow, None),
    ];
    // ⌘1 to ⌘9: the player windows, in the order they opened.
    bindings.extend(
        (1..=9).map(|n| KeyBinding::new(&format!("secondary-{n}"), GoToWindow(n - 1), None)),
    );
    bindings
}

/// Rebind when the shortcuts in Settings change.
pub(super) fn watch_hotkeys(cx: &mut App) {
    cx.observe_global::<Settings>(|cx| {
        let changed = cx
            .try_global::<BoundHotkeys>()
            .is_none_or(|bound| bound.0 != cx.global::<Settings>().hotkeys);
        if changed {
            bind_keys(cx);
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use gpui::{KeyContext, Keymap, Keystroke};

    use super::*;
    use crate::api;
    use crate::app::commands;

    #[test]
    fn command_w_closes_the_file_in_front_or_else_the_window() {
        let keymap = Keymap::new(app_bindings());
        let keys = [Keystroke::parse("secondary-w").unwrap()];
        let root = KeyContext::new_with_defaults();
        let mut workspace = KeyContext::default();
        workspace.add("Workspace");

        let (with_tiles, _) = keymap.bindings_for_input(&keys, &[root.clone(), workspace]);
        assert!(with_tiles[0].action().partial_eq(&CloseFile));
        let (without, _) = keymap.bindings_for_input(&keys, &[root]);
        assert!(without[0].action().partial_eq(&CloseWindow));
    }

    #[test]
    fn the_commands_have_the_keys_bound_to_them() {
        for binding in app_bindings() {
            let invocation = commands::invocation(binding.action()).unwrap();
            let key = binding
                .keystrokes()
                .iter()
                .map(|keystroke| keystroke.unparse())
                .collect::<Vec<_>>()
                .join(" ");
            if !invocation.target.is_empty() {
                // ⌘1 to ⌘9, each aimed at its window.
                assert_eq!(invocation.command, "window.focus", "{key}");
                continue;
            }
            let spec = api::find(&invocation.command).unwrap();
            let keys: Vec<String> = spec
                .default_keys()
                .iter()
                .filter_map(|key| crate::hotkeys::normalize(key))
                .collect();
            assert!(keys.contains(&key), "{} lacks {key}: {keys:?}", spec.id);
        }
    }

    #[test]
    fn the_number_and_tab_keys_go_from_window_to_window_wherever_they_are_pressed() {
        // With the playback shortcuts too, which none of these keys may be.
        let mut bindings = app_bindings();
        bindings.extend(Hotkeys::default().bindings());
        let keymap = Keymap::new(bindings);
        let root = KeyContext::new_with_defaults();
        let mut workspace = KeyContext::default();
        workspace.add("Workspace");
        let in_a_player = [root.clone(), workspace];
        let in_settings = [root];
        let bound = |keys: &str, context: &[KeyContext]| {
            let keys = [Keystroke::parse(keys).unwrap()];
            let (found, _) = keymap.bindings_for_input(&keys, context);
            found.first().map(|binding| binding.action().boxed_clone())
        };

        for context in [&in_a_player[..], &in_settings[..]] {
            for n in 1..=9 {
                let action = bound(&format!("secondary-{n}"), context).unwrap();
                assert!(action.partial_eq(&GoToWindow(n - 1)), "⌘{n}: {action:?}");
            }
            assert!(bound("ctrl-tab", context).unwrap().partial_eq(&NextWindow));
            let back = bound("ctrl-shift-tab", context).unwrap();
            assert!(back.partial_eq(&PreviousWindow));
        }
    }
}
