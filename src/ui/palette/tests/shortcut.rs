//! Changing a command's shortcut in the palette (⌘Return): its keys
//! listed, a key pressed added, one picked removed.

use gpui::{KeyDownEvent, Keystroke, TestAppContext, VisualTestContext, WindowHandle};

use super::HEADING;
use crate::api::{self, Invocation};
use crate::hotkeys::{self, normalize};
use crate::settings::Settings;
use crate::ui::palette::tests::{
    aimed_at, listed, open, open_palette, painted, palette, press_and_wait, read, sections,
    times_painted, type_in,
};
use crate::ui::palette::{COMMAND, Palette};
use crate::ui::settings::SettingsView;
use crate::ui::testing::{self, press, shows};
use crate::ui::workspace::Workspace;

/// `command`'s keys, as saved.
fn keys(cx: &mut VisualTestContext, command: &str) -> Vec<String> {
    cx.update(|_, cx| cx.global::<Settings>().hotkeys.keys(command).to_vec())
}

/// A key as it's saved: ⌘ as cmd on macOS, ctrl elsewhere.
fn key(text: &str) -> String {
    normalize(text).unwrap()
}

/// The command whose keys the palette over `window` changes.
fn changing(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Option<&'static str> {
    read(window, cx, Palette::changing)
}

fn picked(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Option<String> {
    read(window, cx, Palette::picked)
}

/// Open the palette, pick New Window and change its keys with ⌘Return.
fn change_new_window(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) {
    open_palette(cx);
    type_in(cx, "new window");
    press_and_wait(cx, "secondary-enter");
    assert_eq!(changing(window, cx), Some("window.new"));
}

/// Click the row of `key`, while keys are changing.
fn click_key(cx: &mut VisualTestContext, key: &str) {
    let row: &'static str = Box::leak(format!("palette-key-{key}").into_boxed_str());
    testing::click(cx, row);
    cx.run_until_parked();
}

#[gpui::test]
fn command_return_lists_the_keys_of_the_command_picked_and_a_key_pressed_is_added(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    change_new_window(window, &mut cx);
    assert!(palette(window, &cx).is_some(), "it stays up");
    assert!(shows(&mut cx, "palette-asking") && painted(&mut cx, "New Window"));
    assert!(painted(&mut cx, "Press a shortcut for New Window"));
    assert_eq!(
        sections(window, &cx),
        [(HEADING.to_owned(), vec![key("secondary-n")])]
    );
    assert!(painted(&mut cx, &hotkeys::display("secondary-n")));
    assert!(painted(&mut cx, "Remove") && painted(&mut cx, "Back"));
    assert!(!painted(&mut cx, "Run") && !painted(&mut cx, "Change Shortcut"));

    press_and_wait(&mut cx, "secondary-shift-y");
    let added = key("secondary-shift-y");
    assert_eq!(
        keys(&mut cx, "window.new"),
        [key("secondary-n"), added.clone()]
    );
    assert_eq!(changing(window, &cx), Some("window.new"), "still changing");
    assert_eq!(
        sections(window, &cx),
        [(HEADING.to_owned(), vec![key("secondary-n"), added.clone()])]
    );
    assert_eq!(picked(window, &cx), Some(added), "picked");
    assert_eq!(
        times_painted(&mut cx, "Default"),
        1,
        "only the one it starts out with"
    );
    assert!(!shows(&mut cx, "palette-taken"), "no one else's");
    assert!(
        !painted(&mut cx, "Change Shortcut"),
        "whichever key is picked"
    );

    // ⌘Return, which changes keys, is a key like any other here.
    press_and_wait(&mut cx, "secondary-enter");
    assert!(keys(&mut cx, "window.new").contains(&key("secondary-enter")));
}

#[gpui::test]
fn a_key_another_command_has_moves_here_and_says_whose_it_was(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let settings = key("secondary-,");
    assert!(keys(&mut cx, "app.settings.open").contains(&settings));
    change_new_window(window, &mut cx);
    press_and_wait(&mut cx, "secondary-,");
    assert!(keys(&mut cx, "window.new").contains(&settings));
    assert!(!keys(&mut cx, "app.settings.open").contains(&settings));
    assert!(
        !cx.windows()
            .into_iter()
            .any(|window| window.downcast::<SettingsView>().is_some()),
        "recorded, not run"
    );
    let title = api::find("app.settings.open").unwrap().title;
    let note = format!("{} was for “{title}”", hotkeys::display(&settings));
    assert!(
        shows(&mut cx, "palette-taken") && painted(&mut cx, &note),
        "{note}"
    );

    // Held down, it repeats, and says no less.
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse(&settings).unwrap(),
        is_held: true,
    });
    assert!(painted(&mut cx, &note), "{note}");

    press_and_wait(&mut cx, "enter");
    assert!(!keys(&mut cx, "window.new").contains(&settings));
    assert!(!shows(&mut cx, "palette-taken"), "removed, it's no one's");
}

#[gpui::test]
fn keys_a_player_takes_are_recorded_while_changing(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"]);
    cx.update(|_, cx| api::record(cx));
    change_new_window(window, &mut cx);
    press_and_wait(&mut cx, "space");
    assert!(keys(&mut cx, "window.new").contains(&key("space")));
    let ran: Vec<Invocation> = cx.update(|_, cx| api::recorded(cx));
    assert!(
        !ran.iter().any(|ran| ran.command == "playback.play_pause"),
        "{ran:?}"
    );
}

#[gpui::test]
fn return_or_a_click_removes_a_key_and_escape_goes_back(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    change_new_window(window, &mut cx);
    let [n, y, z] = ["secondary-n", "secondary-shift-y", "secondary-shift-z"].map(key);
    press_and_wait(&mut cx, "secondary-shift-y");
    press_and_wait(&mut cx, "secondary-shift-z");
    assert_eq!(listed(window, &cx), [n.clone(), y.clone(), z.clone()]);

    press_and_wait(&mut cx, "up");
    assert_eq!(picked(window, &cx), Some(y.clone()));
    press_and_wait(&mut cx, "enter");
    assert_eq!(keys(&mut cx, "window.new"), [n.clone(), z.clone()]);
    assert_eq!(listed(window, &cx), [n.clone(), z.clone()]);
    assert_eq!(picked(window, &cx), Some(z.clone()), "the one after it");
    assert_eq!(changing(window, &cx), Some("window.new"), "still changing");

    press_and_wait(&mut cx, "up");
    press_and_wait(&mut cx, "enter");
    assert_eq!(keys(&mut cx, "window.new"), std::slice::from_ref(&z));

    click_key(&mut cx, &z);
    assert!(keys(&mut cx, "window.new").is_empty());
    assert!(shows(&mut cx, "palette-empty") && painted(&mut cx, "It has no shortcut"));
    assert!(!painted(&mut cx, "Remove"), "nothing to remove");
    press_and_wait(&mut cx, "secondary-shift-y");

    press_and_wait(&mut cx, "escape");
    assert_eq!(changing(window, &cx), None);
    assert!(palette(window, &cx).is_some(), "back to the commands");
    assert_eq!(picked(window, &cx).as_deref(), Some("window.new"));
    assert!(
        painted(&mut cx, &hotkeys::display(&y)) && !painted(&mut cx, &hotkeys::display(&n)),
        "its row shows the keys it has now"
    );
    type_in(&mut cx, " ");
    assert_eq!(
        picked(window, &cx).as_deref(),
        Some("window.new"),
        "typing goes into the field again"
    );
    press_and_wait(&mut cx, "escape");
    assert!(palette(window, &cx).is_none());
}

#[gpui::test]
fn back_in_the_footer_goes_back_to_the_command_whose_keys_changed(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    type_in(&mut cx, "window");
    let at = listed(window, &cx)
        .iter()
        .position(|id| id == "window.new")
        .expect("New Window is found");
    assert!(at > 0, "not the first found");
    for _ in 0..at {
        press_and_wait(&mut cx, "down");
    }
    testing::click(&mut cx, "palette-change-shortcut");
    cx.run_until_parked();
    assert_eq!(changing(window, &cx), Some("window.new"));
    testing::click(&mut cx, "palette-back");
    cx.run_until_parked();
    assert_eq!(changing(window, &cx), None);
    assert_eq!(picked(window, &cx).as_deref(), Some("window.new"));
    press(&mut cx, "escape");
    cx.run_until_parked();
    assert!(palette(window, &cx).is_none());
}

#[gpui::test]
fn asked_to_show_again_while_changing_it_still_takes_keys(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    change_new_window(window, &mut cx);
    let mut again = Invocation::new(COMMAND);
    again.target = aimed_at(window, &cx);
    cx.update(|_, cx| api::dispatch(again, cx));
    cx.run_until_parked();
    assert_eq!(changing(window, &cx), Some("window.new"));
    press_and_wait(&mut cx, "secondary-shift-y");
    assert!(keys(&mut cx, "window.new").contains(&key("secondary-shift-y")));
}

#[gpui::test]
fn a_click_in_the_palette_leaves_what_is_typed_going_into_the_field(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    let heading = sections(window, &cx)[0].0.clone();
    let heading: &'static str = Box::leak(format!("palette-heading-{heading}").into_boxed_str());
    testing::click(&mut cx, heading);
    cx.run_until_parked();
    type_in(&mut cx, "new window");
    assert_eq!(picked(window, &cx).as_deref(), Some("window.new"));
}
