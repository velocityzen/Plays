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
