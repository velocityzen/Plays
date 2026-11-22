//! The keymap: every command's keys, from Settings (where they start out
//! as the commands' own), bound again whenever Settings change them, and
//! ⌘1 to ⌘9 for the player windows.

use gpui::{App, Global, KeyBinding};

use super::menus;
use crate::api::commands::window;
use crate::hotkeys::Hotkeys;
use crate::settings::Settings;
use crate::ui::{name_sheet, open_url, pairing, palette, text_input};

/// The shortcuts in the keymap, to tell when Settings change them.
struct BoundHotkeys(Hotkeys);
impl Global for BoundHotkeys {}

/// Put the commands' keys in the keymap, and show them in the menus.
pub(super) fn bind_keys(cx: &mut App) {
    let hotkeys = cx.global::<Settings>().hotkeys.clone();
    cx.clear_key_bindings();
    text_input::bind_keys(cx);
    open_url::bind_keys(cx);
    name_sheet::bind_keys(cx);
    pairing::bind_keys(cx);
    palette::bind_keys(cx);
    cx.bind_keys(hotkeys.bindings());
    cx.bind_keys(window_numbers());
    cx.set_global(BoundHotkeys(hotkeys));
    menus::set_menus(cx);
}

/// ⌘1 to ⌘9: the player windows, in the order they opened, wherever
/// they're pressed, but while Settings records a shortcut.
fn window_numbers() -> Vec<KeyBinding> {
    (1..=9)
        .map(|n| {
            KeyBinding::new(
                &format!("secondary-{n}"),
                window::focus(n),
                Some("!Recording"),
            )
        })
        .collect()
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
    use crate::api::Run;

    #[test]
    fn the_number_and_tab_keys_go_from_window_to_window_wherever_they_are_pressed() {
        // With every command's keys, which none of these may be.
        let mut bindings = Hotkeys::default().bindings();
        bindings.extend(window_numbers());
        let keymap = Keymap::new(bindings);
        let root = KeyContext::new_with_defaults();
        let mut workspace = KeyContext::default();
        workspace.add("Workspace");
        let in_a_player = [root.clone(), workspace];
        let in_settings = [root.clone(), KeyContext::parse("Settings").unwrap()];
        let recording = [root, KeyContext::parse("Settings Recording").unwrap()];
        // An empty player window sets none.
        let empty: [KeyContext; 0] = [];
        let bound = |keys: &str, context: &[KeyContext]| {
            let keys = [Keystroke::parse(keys).unwrap()];
            let (found, _) = keymap.bindings_for_input(&keys, context);
            found.first().map(|binding| binding.action().boxed_clone())
        };

        for context in [&in_a_player[..], &in_settings[..], &empty[..]] {
            for n in 1..=9 {
                let action = bound(&format!("secondary-{n}"), context).unwrap();
                assert!(action.partial_eq(&window::focus(n)), "⌘{n}: {action:?}");
            }
            let next = bound("ctrl-tab", context).unwrap();
            assert!(next.partial_eq(&Run::command("window.next")));
            let back = bound("ctrl-shift-tab", context).unwrap();
            assert!(back.partial_eq(&Run::command("window.previous")));
        }
        assert!(
            bound("secondary-1", &recording).is_none(),
            "Settings takes it"
        );
    }
}
