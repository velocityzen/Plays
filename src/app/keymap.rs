//! The keymap: the app's own shortcuts and the playback ones from Settings,
//! bound again whenever Settings change them.

use gpui::{App, Global, KeyBinding};

use super::{CloseAll, CloseFile, CloseWindow, OpenFiles, OpenSettings, OpenUrl, Quit, menus};
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
    vec![
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
    ]
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
}
