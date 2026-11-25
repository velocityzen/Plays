//! The window commands, run as the API runs them: opening an empty window,
//! closing the others, naming one, going back to one in front before, and
//! moving one in the ⌘1 to ⌘9 numbering; and a closed one, which its
//! platform may still call about.

use gpui::{TestAppContext, VisualTestContext, WindowHandle, WindowId};
use serde_json::{Value, json};

use crate::api::{self, Error, Invocation, Target};
use crate::app::player_windows;
use crate::ui::settings::SettingsView;
use crate::ui::testing::{self, press, shows};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// Run `invocation` as a deferred command does, outside any window's
/// update, and what it hands back.
fn run(cx: &mut VisualTestContext, invocation: Invocation) -> Result<Value, Error> {
    let done = TestAppContext::update(cx, |cx| api::run(invocation, cx));
    cx.run_until_parked();
    done
}

/// `command`, given `args`, aimed at `window`.
fn on(
    command: &str,
    args: Value,
    window: WindowHandle<Workspace>,
    cx: &VisualTestContext,
) -> Invocation {
    Invocation::new(command)
        .with_args(args)
        .in_window(name(window, cx))
}

/// The API's name for `window`.
fn name(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> String {
    window
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap()
}

/// The player window the API names `name`.
fn named(name: &str, cx: &VisualTestContext) -> WindowHandle<Workspace> {
    cx.windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Workspace>())
        .find(|window| {
            window
                .read_with(cx, |workspace, _| workspace.id().to_string() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no window {name}"))
}

/// Open another player window, behind the one in front, as the test
/// platform leaves new windows.
fn open_player(cx: &mut VisualTestContext) -> WindowHandle<Workspace> {
    let window = TestAppContext::update(cx, |cx| Workspace::open_window(WINDOW_SIZE, cx))
        .expect("opening a window");
    cx.run_until_parked();
    window
}

fn bring_to_front(window: WindowHandle<Workspace>, cx: &VisualTestContext) {
    let mut window = VisualTestContext::from_window(window.into(), cx);
    window.update(|window, _| window.activate_window());
    window.run_until_parked();
}

fn front(cx: &VisualTestContext) -> Option<WindowId> {
    TestAppContext::read(cx, |cx| cx.active_window().map(|window| window.window_id()))
}

/// The player windows, in the order ⌘1 to ⌘9 count them.
fn numbered(cx: &VisualTestContext) -> Vec<WindowId> {
    TestAppContext::read(cx, |cx| {
        player_windows(cx)
            .iter()
            .map(|window| window.window_id())
            .collect()
    })
}

/// `windows`, as `numbered` has them.
fn ids(windows: &[WindowHandle<Workspace>]) -> Vec<WindowId> {
    windows.iter().map(|window| window.window_id()).collect()
}

fn label(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Option<String> {
    window
        .read_with(cx, |workspace, _| workspace.label().map(str::to_owned))
        .unwrap()
}

fn title(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Option<String> {
    VisualTestContext::from_window(window.into(), cx).window_title()
}

/// Press `keys` in `window`.
fn press_in(window: WindowHandle<Workspace>, cx: &VisualTestContext, keys: &str) {
    let mut window = VisualTestContext::from_window(window.into(), cx);
    press(&mut window, keys);
    window.run_until_parked();
}

#[gpui::test]
fn new_opens_an_empty_window_in_front_numbered_last(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);

    let opened = run(&mut cx, Invocation::new("window.new")).unwrap();
    let new = named(opened["window"].as_str().expect("its window"), &cx);
    assert_eq!(numbered(&cx), ids(&[first, new]));
    assert_eq!(front(&cx), Some(new.window_id()), "in front");
    let empty = new
        .read_with(&cx, |workspace, _| workspace.players().is_empty())
        .unwrap();
    assert!(empty, "with nothing in it");

    press_in(new, &cx, "secondary-n");
    assert_eq!(numbered(&cx).len(), 3, "⌘N opens another");
}

#[gpui::test]
fn close_others_closes_every_player_window_but_the_one_named(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let second = open_player(&mut cx);
    open_player(&mut cx);
    TestAppContext::update(&cx, SettingsView::open_window);
    cx.run_until_parked();

    let invocation = on("window.close_others", Value::Null, second, &cx);
    run(&mut cx, invocation).unwrap();
    assert_eq!(numbered(&cx), ids(&[second]));
    let settings = cx
        .windows()
        .iter()
        .any(|window| window.downcast::<SettingsView>().is_some());
    assert!(settings, "Settings isn't a player window, and stays");
}

#[gpui::test]
fn rename_labels_a_window_its_title_shows_and_names_it_by(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    let second = open_player(&mut cx);

    let invocation = on("window.rename", json!({"label": "  Lobby "}), second, &cx);
    run(&mut cx, invocation).unwrap();
    assert_eq!(label(second, &cx).as_deref(), Some("Lobby"), "trimmed");
    assert_eq!(title(second, &cx).as_deref(), Some("Lobby"));
    run(&mut cx, Invocation::new("window.focus").in_window("Lobby")).unwrap();
    assert_eq!(front(&cx), Some(second.window_id()), "named by it");

    let invocation = on("window.rename", json!({"label": "Lobby"}), second, &cx);
    run(&mut cx, invocation).expect("its own label again");
    let invocation = on("window.rename", json!({"label": "Lobby"}), first, &cx);
    assert_eq!(
        run(&mut cx, invocation),
        Err(Error::bad_args(
            "window.rename",
            "another window is called Lobby"
        ))
    );
    assert_eq!(label(first, &cx), None, "another's label isn't taken");

    let invocation = on("window.rename", json!({"label": " "}), second, &cx);
    run(&mut cx, invocation).unwrap();
    assert_eq!(label(second, &cx), None, "blank is none");
    assert_eq!(title(second, &cx).as_deref(), Some("Plays"), "as it was");
}

#[gpui::test]
fn rename_with_no_label_asks_for_one(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let other = open_player(&mut cx);
    let invocation = on("window.rename", json!({"label": "Stage"}), other, &cx);
    run(&mut cx, invocation).unwrap();

    let invocation = on("window.rename", Value::Null, window, &cx);
    run(&mut cx, invocation).unwrap();
    assert!(shows(&mut cx, "rename-window"), "it asks");
    cx.simulate_input("Stage");
    press(&mut cx, "enter");
    assert!(
        shows(&mut cx, "rename-window-error"),
        "another window is called so"
    );
    assert!(shows(&mut cx, "rename-window"), "and it asks again");
    assert_eq!(label(window, &cx), None);

    press(&mut cx, "backspace backspace backspace backspace backspace");
    cx.simulate_input("Hall");
    press(&mut cx, "enter");
    cx.run_until_parked();
    assert!(!shows(&mut cx, "rename-window"), "named, it goes");
    assert_eq!(label(window, &cx).as_deref(), Some("Hall"));
    assert_eq!(title(window, &cx).as_deref(), Some("Hall"));

    // Asked again, Escape leaves the name as it is.
    let invocation = on("window.rename", Value::Null, window, &cx);
    run(&mut cx, invocation).unwrap();
    assert!(shows(&mut cx, "rename-window"));
    press(&mut cx, "escape");
    assert!(!shows(&mut cx, "rename-window"), "Escape puts it away");
    assert_eq!(label(window, &cx).as_deref(), Some("Hall"));
}

#[gpui::test]
fn back_walks_back_through_the_windows_in_front_and_recent_goes_to_the_last(
    cx: &mut TestAppContext,
) {
    let (a, mut cx) = testing::start(cx);
    let b = open_player(&mut cx);
    let c = open_player(&mut cx);
    let d = open_player(&mut cx);
    // Each brought to the front in turn, `a` from behind another.
    for window in [b, a, b, c, d] {
        bring_to_front(window, &cx);
    }
    let back = |cx: &mut VisualTestContext| run(cx, Invocation::new("window.back")).unwrap();
    let recent = |cx: &mut VisualTestContext| run(cx, Invocation::new("window.recent")).unwrap();

    back(&mut cx);
    assert_eq!(front(&cx), Some(c.window_id()), "back to the one before");
    back(&mut cx);
    assert_eq!(front(&cx), Some(b.window_id()), "and on back, past it");
    back(&mut cx);
    assert_eq!(front(&cx), Some(a.window_id()));
    back(&mut cx);
    assert_eq!(
        front(&cx),
        Some(a.window_id()),
        "none was in front before it"
    );

    // Another in front, the walk starts again, even back where it went.
    bring_to_front(d, &cx);
    bring_to_front(a, &cx);
    back(&mut cx);
    assert_eq!(front(&cx), Some(d.window_id()), "in front just before");
    // Another command, it starts again too.
    run(&mut cx, Invocation::new("window.float_on_top.toggle")).unwrap();
    back(&mut cx);
    assert_eq!(front(&cx), Some(a.window_id()), "not on to b");

    recent(&mut cx);
    assert_eq!(front(&cx), Some(d.window_id()));
    recent(&mut cx);
    assert_eq!(front(&cx), Some(a.window_id()), "and back again");
    press_in(a, &cx, "secondary-'");
    assert_eq!(front(&cx), Some(d.window_id()), "⌘'");

    // One closed is passed over.
    bring_to_front(d, &cx);
    a.update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    bring_to_front(d, &cx);
    recent(&mut cx);
    assert_eq!(front(&cx), Some(b.window_id()), "the last open one");
}

#[gpui::test]
fn move_puts_a_window_at_another_number(cx: &mut TestAppContext) {
    let (a, mut cx) = testing::start(cx);
    let b = open_player(&mut cx);
    let c = open_player(&mut cx);

    let invocation = on("window.move", json!({"position": 1}), c, &cx);
    run(&mut cx, invocation).unwrap();
    assert_eq!(numbered(&cx), ids(&[c, a, b]));
    press_in(a, &cx, "secondary-1");
    assert_eq!(front(&cx), Some(c.window_id()), "⌘1 goes to it");
    let place = TestAppContext::update(&cx, |cx| {
        let target = Target {
            window: Some("1".into()),
            block: None,
        };
        api::state("window.move", &target, cx)
    });
    assert_eq!(place, Ok(json!(1)), "its number");

    for (window, command, after, why) in [
        (c, "window.move.forward", [a, c, b], "one later"),
        (c, "window.move.forward", [a, b, c], "the last"),
        (c, "window.move.forward", [a, b, c], "the last stays so"),
        (a, "window.move.backward", [a, b, c], "the first stays so"),
        (c, "window.move.backward", [a, c, b], "one earlier"),
    ] {
        let invocation = on(command, Value::Null, window, &cx);
        run(&mut cx, invocation).unwrap();
        assert_eq!(numbered(&cx), ids(&after), "{why}");
    }

    let invocation = on("window.move", json!({"position": 4}), c, &cx);
    assert_eq!(
        run(&mut cx, invocation),
        Err(Error::bad_args(
            "window.move",
            "position is at most 3, as many as there are windows"
        ))
    );

    // Opened, one is numbered last; closed, the others keep their order.
    let d = open_player(&mut cx);
    assert_eq!(numbered(&cx), ids(&[a, c, b, d]));
    c.update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(numbered(&cx), ids(&[a, b, d]));
}

/// A window's platform can still call about it once it has closed, as
/// macOS does till it closes its own window, and the test platform does,
/// telling the window in front before that it no longer is. With no window
/// to update, that's nothing to do, not an error.
#[gpui::test]
fn the_platform_calling_about_a_closed_window_logs_no_error(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    let second = open_player(&mut cx);
    bring_to_front(first, &cx);
    testing::keep_errors();

    let invocation = on("window.close", Value::Null, first, &cx);
    run(&mut cx, invocation).unwrap();
    bring_to_front(second, &cx);
    assert_eq!(numbered(&cx), ids(&[second]));
    assert_eq!(front(&cx), Some(second.window_id()));
    assert_eq!(testing::errors_logged(), Vec::<String>::new());
}

/// The platform can still have a closed window in front, as macOS does till
/// it closes its own; a menu's command then runs as with none in front.
#[gpui::test]
fn a_menus_command_runs_with_a_closed_window_still_in_front(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    open_player(&mut cx);
    bring_to_front(first, &cx);
    let invocation = on("window.close", Value::Null, first, &cx);
    run(&mut cx, invocation).unwrap();
    testing::keep_errors();

    let new = api::Run(Invocation::new("window.new"));
    TestAppContext::update(&cx, |cx| cx.dispatch_action(&new));
    cx.run_until_parked();
    assert_eq!(numbered(&cx).len(), 2, "a window opens");
    assert_eq!(testing::errors_logged(), Vec::<String>::new());
}
