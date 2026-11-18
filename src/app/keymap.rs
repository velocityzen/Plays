//! The keymap: the app's own shortcuts and the playback ones from Settings,
//! bound again whenever Settings change them.

use gpui::{App, Global, KeyBinding};

use super::{CloseAll, CloseWindow, OpenFiles, OpenSettings, OpenUrl, Quit, menus};
use crate::hotkeys::Hotkeys;
use crate::settings::Settings;
use crate::ui::{open_url, text_input};

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
    cx.bind_keys([
        KeyBinding::new("secondary-o", OpenFiles, None),
        KeyBinding::new("secondary-shift-o", OpenUrl, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-w", CloseWindow, None),
        KeyBinding::new("secondary-shift-w", CloseAll, Some("Workspace")),
    ]);
    cx.bind_keys(hotkeys.bindings());
    cx.set_global(BoundHotkeys(hotkeys));
    menus::set_menus(cx);
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
