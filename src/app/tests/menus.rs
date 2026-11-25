use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Bounds, TestAppContext, VisualTestContext, WindowHandle};

use super::*;
use crate::api::Invocation;
use crate::cast::fake::FakeTarget;
use crate::ui::testing::{self, OPEN_WAIT};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// Each action item's name and whether it is checked.
fn listed(menu: &Menu) -> Vec<(String, bool)> {
    menu.items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action { name, checked, .. } => Some((name.to_string(), *checked)),
            _ => None,
        })
        .collect()
}

fn device(id: &str, name: &str) -> DeviceInfo {
    DeviceInfo {
        id: id.into(),
        name: name.into(),
    }
}

/// The items checked in `menu`.
fn checked(menu: &Menu) -> Vec<String> {
    listed(menu)
        .into_iter()
        .filter(|(_, checked)| *checked)
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn lists_the_chapters_checking_the_one_the_block_is_in() {
    assert!(
        chapter_menu(&[], &Value::Null).is_none(),
        "none for a file without"
    );
    let names = ["Opening".to_owned(), "Middle".to_owned()];
    let menu = chapter_menu(&names, &json!(2)).unwrap();
    assert_eq!(
        listed(&menu),
        [("Opening".into(), false), ("Middle".into(), true)]
    );
}

#[gpui::test]
fn the_plays_menu_installs_the_command_line_tool_after_settings(cx: &mut TestAppContext) {
    let (_, cx) = testing::start(cx);
    let state = TestAppContext::update(&cx, MenuState::now);
    let names: Vec<String> = listed(&plays_menu(&state))
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let mut expected = vec!["About Plays", "Check for Updates…", "Settings…"];
    // Elsewhere Plays is installed where the system keeps programs.
    if cfg!(target_os = "macos") {
        expected.push("Install Command Line Tool");
    }
    expected.push("Quit Plays");
    assert_eq!(names, expected);
}

#[gpui::test]
fn lists_the_displays_as_last_looked_at(cx: &mut TestAppContext) {
    let (_, cx) = testing::start(cx);
    // Looked at every second, not each time the menus are made: asking
    // is slow on X11.
    TestAppContext::update(&cx, |cx| {
        let mut seen = displays::list(cx);
        seen.push(display(7, "Projector"));
        displays::remember(seen, cx);
    });
    let state = TestAppContext::update(&cx, MenuState::now);
    assert_eq!(
        state.displays.last().map(|display| display.name.as_str()),
        Some("Projector"),
        "{:?}",
        state.displays
    );
}

#[gpui::test]
fn the_chapters_menu_follows_the_block_in_front(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![testing::fixture("clip-chapters.mkv")], cx)
        })
        .unwrap();
    let known = testing::wait_until(&mut cx, |cx| {
        let state = TestAppContext::update(cx, MenuState::now);
        !state.chapters.is_empty()
    });
    assert!(known, "its chapters are known");
    let state = TestAppContext::update(&cx, MenuState::now);
    assert_eq!(state.chapters, ["Opening", "Middle", "End"]);
    assert_eq!(state.state("playback.go_to_chapter"), &json!(1));

    cx.dispatch_action(playback::go_to_chapter(3));
    cx.run_until_parked();
    let shown = cx.update(|_, cx| {
        let state = cx.try_global::<MenuState>().cloned().unwrap_or_default();
        state.state("playback.go_to_chapter").clone()
    });
    assert_eq!(shown, json!(3), "at once");
}

#[test]
fn the_video_menu_sizes_the_window_for_its_videos() {
    let menu = video_menu(&MenuState::default());
    let matches = menu.items.iter().any(|item| match item {
        MenuItem::Action { name, action, .. } => {
            name == "Match Video Size"
                && action.partial_eq(&Run::command("window.match_video_size"))
        }
        _ => false,
    });
    assert!(matches, "no Match Video Size in the Video menu");
}

#[test]
fn the_interface_menu_shows_or_hides_the_panels_in_the_block_its_window_and_every_window() {
    let mut state = MenuState::default();
    for (command, on) in [
        ("block.toggle_controls", true),
        ("block.toggle_top_bar", false),
        ("block.toggle_track_switcher", true),
        ("block.toggle_notes", false),
        ("window.toggle_notes", true),
        ("app.toggle_top_bar", true),
    ] {
        state.states.insert(command, json!(on));
    }
    let menu = interface_menu(&state);
    assert_eq!(menu.name, "Interface");
    let separators: Vec<usize> = menu
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, MenuItem::Separator))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(separators, [5, 11], "one group each");
    let panel = |name: &str, on| (name.to_owned(), on);
    assert_eq!(
        listed(&menu),
        [
            panel("Show/Hide All", false),
            panel("Show Controls", true),
            panel("Show Top Bar", false),
            panel("Show Track Switcher", true),
            panel("Show Notes", false),
            panel("Show/Hide All in Window", false),
            panel("Show Controls in Window", false),
            panel("Show Top Bar in Window", false),
            panel("Show Track Switcher in Window", false),
            panel("Show Notes in Window", true),
            panel("Show/Hide All in Every Window", false),
            panel("Show Controls in Every Window", false),
            panel("Show Top Bar in Every Window", true),
            panel("Show Track Switcher in Every Window", false),
            panel("Show Notes in Every Window", false),
        ]
    );
    let video = listed(&video_menu(&state));
    assert!(
        video.iter().all(|(name, _)| !name.starts_with("Show ")),
        "not in the Video menu too: {video:?}"
    );
}

#[gpui::test]
fn the_interface_menu_comes_before_the_window_menu(cx: &mut TestAppContext) {
    let (_, cx) = testing::start(cx);
    let names: Vec<String> = TestAppContext::update(&cx, |cx| {
        menus(cx).into_iter().map(|menu| menu.name.into()).collect()
    });
    let at = |name: &str| names.iter().position(|menu| menu == name);
    assert_eq!(at("Interface").map(|at| at + 1), at("Window"), "{names:?}");
}

#[test]
fn the_window_menu_goes_to_the_next_window_or_the_one_before() {
    let menu = window_menu(&MenuState::default());
    let has = |title: &str, command: &str| {
        menu.items.iter().any(|item| match item {
            MenuItem::Action { name, action, .. } => {
                name == title && action.partial_eq(&Run::command(command))
            }
            _ => false,
        })
    };
    assert!(has("Next Window", "window.next"), "no Next Window");
    assert!(
        has("Previous Window", "window.previous"),
        "no Previous Window"
    );
}

#[test]
fn open_recent_opens_the_files_played_lately_by_name_and_folder_where_shared() {
    let none = file_menu(&MenuState::default());
    fn recent(menu: &Menu) -> Option<&Menu> {
        menu.items.iter().find_map(|item| match item {
            MenuItem::Submenu(menu) if menu.name == "Open Recent" => Some(menu),
            _ => None,
        })
    }
    assert!(recent(&none).is_none(), "none without any");

    let state = MenuState {
        recent_files: ["/media/a/x.mkv", "/media/b/x.mkv", "/media/b/y.mp4"]
            .map(str::to_owned)
            .into(),
        ..MenuState::default()
    };
    let file = file_menu(&state);
    let at = file
        .items
        .iter()
        .position(|item| matches!(item, MenuItem::Submenu(_)));
    assert_eq!(at, Some(2), "after Open URL…");
    let menu = recent(&file).expect("Open Recent");
    let items: Vec<(String, Option<Run>)> = menu
        .items
        .iter()
        .map(|item| match item {
            MenuItem::Action { name, action, .. } => (
                name.to_string(),
                action.as_any().downcast_ref::<Run>().cloned(),
            ),
            _ => (String::new(), None),
        })
        .collect();
    let open = |file: &str| Some(Run::with("app.open", json!({"urls": [file]})));
    assert_eq!(
        items,
        [
            ("x.mkv — /media/a".to_owned(), open("/media/a/x.mkv")),
            ("x.mkv — /media/b".to_owned(), open("/media/b/x.mkv")),
            ("y.mp4".to_owned(), open("/media/b/y.mp4")),
            (String::new(), None),
            (
                "Clear Recent Files".to_owned(),
                Some(Run::command("app.clear_recent_files"))
            ),
        ]
    );
}

#[gpui::test]
fn the_menus_follow_the_files_played_lately(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    cx.update(|_, cx| crate::recents::FILES.note("/media/a.mkv", cx));
    cx.run_until_parked();
    let shown = cx.update(|_, cx| cx.global::<MenuState>().recent_files.clone());
    assert_eq!(shown, ["/media/a.mkv"]);
}

#[gpui::test]
fn clear_recent_files_forgets_them_and_the_menu_follows(cx: &mut TestAppContext) {
    let (_, cx) = testing::start(cx);
    TestAppContext::update(&cx, |cx| {
        crate::recents::FILES.note("/media/a.mkv", cx);
        assert_eq!(MenuState::now(cx).recent_files, ["/media/a.mkv"]);
        api::run(api::Invocation::new("app.clear_recent_files"), cx).unwrap();
        assert!(crate::recents::FILES.list(cx).is_empty());
        assert!(MenuState::now(cx).recent_files.is_empty());
    });
}

#[test]
fn the_window_menu_opens_names_closes_goes_back_to_and_moves_windows() {
    let state = MenuState {
        windows: 3,
        states: BTreeMap::from([("window.move", json!(2))]),
        ..MenuState::default()
    };
    let menu = window_menu(&state);
    let has = |title: &str, command: &str| {
        menu.items.iter().any(|item| match item {
            MenuItem::Action { name, action, .. } => {
                name == title && action.partial_eq(&Run::command(command))
            }
            _ => false,
        })
    };
    assert!(
        matches!(&menu.items[0], MenuItem::Action { name, action, .. }
            if name == "Command Palette…"
                && action.partial_eq(&Run::command("window.command_palette"))),
        "the palette first"
    );
    for (title, command) in [
        ("Go to…", "window.go_to"),
        ("New Window", "window.new"),
        ("Rename Window…", "window.rename"),
        ("Close Other Windows", "window.close_others"),
        ("Go Back One Window", "window.back"),
        ("Go to Recent Window", "window.recent"),
        ("Move Window Backward", "window.move.backward"),
        ("Move Window Forward", "window.move.forward"),
    ] {
        assert!(has(title, command), "no {title}");
    }

    // A number for each window, the one in front's checked.
    let numbers = menu.items.iter().find_map(|item| match item {
        MenuItem::Submenu(menu) if menu.name == "Move Window To" => Some(menu),
        _ => None,
    });
    let numbers = numbers.expect("no Move Window To");
    let items: Vec<(String, bool)> = numbers
        .items
        .iter()
        .map(|item| match item {
            MenuItem::Action {
                name,
                action,
                checked,
                ..
            } => {
                let number: usize = name.parse().expect("a number");
                assert!(action.partial_eq(&window::move_to(number)), "{name}");
                (name.to_string(), *checked)
            }
            _ => panic!("a number"),
        })
        .collect();
    let expected = [("1", false), ("2", true), ("3", false)].map(|(n, c)| (n.to_owned(), c));
    assert_eq!(items, expected);

    let alone = window_menu(&MenuState {
        windows: 1,
        ..MenuState::default()
    });
    let numbered = alone
        .items
        .iter()
        .any(|item| matches!(item, MenuItem::Submenu(menu) if menu.name == "Move Window To"));
    assert!(!numbered, "nowhere to move one alone");
}

#[test]
fn the_menus_close_copy_maximize_name_and_move_the_block_in_front() {
    let state = MenuState {
        blocks: 3,
        others: vec![
            ("a1".into(), "Window 1".into()),
            ("c3".into(), "Stage".into()),
        ],
        states: BTreeMap::from([("block.move", json!(3))]),
        ..MenuState::default()
    };
    let has = |menu: &Menu, title: &str, command: &str| {
        menu.items.iter().any(|item| match item {
            MenuItem::Action { name, action, .. } => {
                name == title && action.partial_eq(&Run::command(command))
            }
            _ => false,
        })
    };
    let file = file_menu(&state);
    for (title, command) in [
        ("Close Other Files", "block.close_others"),
        ("Copy File Path", "block.copy_path"),
        ("Copy Current Time", "block.copy_time"),
    ] {
        assert!(has(&file, title, command), "no {title}");
    }
    let window = window_menu(&state);
    for (title, command) in [
        ("Maximize Block", "block.zoom"),
        ("Rename Block…", "block.rename"),
        ("Move Block to New Window", "block.move_to_new_window"),
        ("Go Back One Block", "block.back"),
        ("Go to Recent Block", "block.recent"),
    ] {
        assert!(has(&window, title, command), "no {title}");
    }

    let submenu = |menu: &Menu, title: &str| -> Option<Vec<(String, bool, Run)>> {
        menu.items.iter().find_map(|item| match item {
            MenuItem::Submenu(menu) if menu.name == title => Some(
                menu.items
                    .iter()
                    .map(|item| match item {
                        MenuItem::Action {
                            name,
                            action,
                            checked,
                            ..
                        } => {
                            let run = action.as_any().downcast_ref::<Run>().expect("a Run");
                            (name.to_string(), *checked, run.clone())
                        }
                        _ => panic!("an item"),
                    })
                    .collect(),
            ),
            _ => None,
        })
    };
    // A place for each block, the one in front's checked.
    let places = submenu(&window, "Move Block To").expect("no Move Block To");
    let expected: Vec<(String, bool, Run)> = [(1, false), (2, false), (3, true)]
        .into_iter()
        .map(|(place, checked)| (place.to_string(), checked, block::move_to(place)))
        .collect();
    assert_eq!(places, expected);
    // The other windows, by label or number.
    let others = submenu(&window, "Move Block to Window").expect("no Move Block to Window");
    let expected = vec![
        ("Window 1".to_owned(), false, block::move_to_window("a1")),
        ("Stage".to_owned(), false, block::move_to_window("c3")),
    ];
    assert_eq!(others, expected);

    // A block alone has nowhere else to go in its window; a window alone,
    // no other window; and none in front, nothing to move.
    let alone = window_menu(&MenuState {
        blocks: 1,
        ..state.clone()
    });
    assert!(submenu(&alone, "Move Block To").is_none());
    assert!(submenu(&alone, "Move Block to Window").is_some());
    let one_window = window_menu(&MenuState {
        others: Vec::new(),
        ..state.clone()
    });
    assert!(submenu(&one_window, "Move Block to Window").is_none());
    let empty = window_menu(&MenuState { blocks: 0, ..state });
    assert!(submenu(&empty, "Move Block to Window").is_none());
}

#[test]
fn checks_the_aspect_ratio_and_crop_of_the_block_in_front() {
    let state = MenuState::default();
    assert_eq!(
        checked(&aspect_menu(&Value::Null, &state)),
        ["As the File Is"]
    );
    let ratio = ASPECT_RATIOS[1];
    assert_eq!(checked(&aspect_menu(&json!(ratio.name), &state)), ["4:3"]);
    assert_eq!(checked(&crop_menu(&Value::Null, &state)), ["None"]);
    let crop = CROPS[CROPS.len() - 1];
    assert_eq!(checked(&crop_menu(&json!(crop.name), &state)), [crop.name]);
}

#[gpui::test]
fn the_menus_know_the_blocks_in_front_and_the_other_windows(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    let now = |cx: &VisualTestContext| TestAppContext::update(cx, MenuState::now);
    let open = |cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| Workspace::open_window(WINDOW_SIZE, cx)).unwrap()
    };
    let name = |window: WindowHandle<Workspace>, cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.id().to_string())
            .unwrap()
    };
    let second = open(&mut cx);
    let third = open(&mut cx);
    first
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![testing::fixture("clip.mp4")], cx);
        })
        .unwrap();
    cx.run_until_parked();

    let state = now(&cx);
    assert_eq!(state.blocks, 1, "the window in front's");
    let others = |names: [(WindowHandle<Workspace>, &str); 2], cx: &VisualTestContext| {
        names.map(|(window, title)| (name(window, cx), title.to_owned()))
    };
    let expected = others([(second, "Window 2"), (third, "Window 3")], &cx);
    assert_eq!(
        state.others, expected,
        "by number, the one in front left out"
    );

    second
        .update(&mut cx, |workspace, window, cx| {
            workspace.set_label("Stage", window, cx)
        })
        .unwrap()
        .unwrap();
    let expected = others([(second, "Stage"), (third, "Window 3")], &cx);
    assert_eq!(now(&cx).others, expected, "by label");
}

#[gpui::test]
fn the_menus_number_the_windows_as_they_open_and_close(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let shown = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.update(|_, cx| {
            let state = cx.try_global::<MenuState>().cloned().unwrap_or_default();
            (state.windows, state.state("window.move").clone())
        })
    };
    let open = |cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| Workspace::open_window(WINDOW_SIZE, cx)).unwrap()
    };

    open(&mut cx);
    let third = open(&mut cx);
    assert_eq!(shown(&mut cx), (3, json!(1)), "the first in front");
    third
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    assert_eq!(shown(&mut cx).0, 2, "one closed");
}

#[gpui::test]
fn the_menus_follow_the_picture_of_the_block_in_front_at_once(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    window
        .update(&mut cx, |workspace, _, cx| {
            let files = ["clip.mp4", "clip-ip.mkv"].map(testing::fixture);
            workspace.open_paths(files.into(), cx)
        })
        .unwrap();
    let start = Instant::now();
    while !testing::shows(&mut cx, "block-1") {
        assert!(start.elapsed() < OPEN_WAIT, "the files didn't open");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Each block's media known, the mouse over it.
    for block in ["block-1", "block-0"] {
        testing::hover(&mut cx, block);
        let known = testing::wait_until(&mut cx, |cx| {
            let mut known = false;
            window
                .update(cx, |workspace, _, cx| {
                    workspace.with_active(cx, |view, _| known = view.player().is_known())
                })
                .unwrap();
            known
        });
        assert!(known, "{block}'s media is known");
    }
    let shown = |cx: &mut VisualTestContext| {
        let state = cx.update(|_, cx| cx.try_global::<MenuState>().cloned());
        state.map(|state| {
            let read = |command| state.state(command).clone();
            (read("video.set_aspect_ratio"), read("video.set_crop"))
        })
    };
    let crop = CROPS[0];
    let ratio = json!(ASPECT_RATIOS[0].name);
    let none = Value::Null;

    cx.dispatch_action(video::set_crop(Some(crop)));
    cx.run_until_parked();
    assert_eq!(shown(&mut cx), Some((none.clone(), json!(crop.name))));

    testing::hover(&mut cx, "block-1");
    assert_eq!(
        shown(&mut cx),
        Some((none.clone(), none.clone())),
        "the other block's"
    );
    cx.dispatch_action(Run::command("video.next_aspect_ratio"));
    cx.run_until_parked();
    assert_eq!(shown(&mut cx), Some((ratio.clone(), none.clone())));

    testing::hover(&mut cx, "block-0");
    assert_eq!(shown(&mut cx), Some((none.clone(), json!(crop.name))));

    // Gone to from the keyboard: the other block's again.
    cx.dispatch_action(Run::command("block.focus.next"));
    cx.run_until_parked();
    assert_eq!(shown(&mut cx), Some((ratio, none)));
}

#[test]
fn checks_the_output_picked() {
    let devices = [device("a", "Speakers"), device("b", "Headphones")];
    assert_eq!(
        listed(&output_menu(&devices, None, &MenuState::default())),
        [
            ("System Default".into(), true),
            ("Speakers".into(), false),
            ("Headphones".into(), false),
        ]
    );
    let picked = AudioDevice {
        id: "b".into(),
        name: "Headphones".into(),
    };
    assert_eq!(
        listed(&output_menu(&devices, Some(&picked), &MenuState::default())),
        [
            ("System Default".into(), false),
            ("Speakers".into(), false),
            ("Headphones".into(), true),
        ]
    );
}

fn display(id: u32, name: &str) -> DisplayInfo {
    DisplayInfo {
        id,
        name: name.into(),
        bounds: Bounds::default(),
    }
}

fn tv(id: &str, name: &str) -> Device {
    Device {
        kind: "Fake",
        id: id.into(),
        name: name.into(),
        shows: Shows::Video,
        address: std::net::Ipv4Addr::LOCALHOST.into(),
    }
}

fn speaker(id: &str, name: &str) -> Device {
    Device {
        shows: Shows::Sound,
        ..tv(id, name)
    }
}

#[test]
fn lists_the_speakers_after_the_outputs_and_checks_the_one_cast_to_instead() {
    let devices = [device("a", "Headphones")];
    let mut outputs = MenuState {
        speakers: vec![speaker("k", "Kitchen"), speaker("p", "Phantom")],
        ..MenuState::default()
    };
    assert_eq!(
        listed(&output_menu(&devices, None, &outputs)),
        [
            ("System Default".into(), true),
            ("Headphones".into(), false),
            ("Kitchen".into(), false),
            ("Phantom".into(), false),
        ]
    );
    outputs.casting = Some(speaker("p", "Phantom"));
    assert_eq!(
        listed(&output_menu(&devices, None, &outputs)),
        [
            ("System Default".into(), false),
            ("Headphones".into(), false),
            ("Kitchen".into(), false),
            ("Phantom".into(), true),
        ]
    );
    // A TV it casts to plays its sound too.
    outputs.casting = Some(tv("t", "Living Room"));
    assert_eq!(
        listed(&output_menu(&devices, None, &outputs))
            .last()
            .cloned(),
        Some(("Living Room".into(), true))
    );
}

#[test]
fn a_speaker_cast_to_leaves_the_picture_on_its_display() {
    let outputs = MenuState {
        displays: vec![display(1, "Built-in Display")],
        front: Some(1),
        devices: vec![tv("t", "Living Room")],
        speakers: vec![speaker("k", "Kitchen")],
        casting: Some(speaker("k", "Kitchen")),
        ..MenuState::default()
    };
    assert_eq!(
        listed(&display_menu(&outputs)),
        [
            ("Built-in Display".into(), true),
            ("Living Room".into(), false),
        ]
    );
}

#[test]
fn checks_the_display_the_window_in_front_is_on() {
    let outputs = MenuState {
        displays: vec![display(1, "Built-in Display"), display(7, "Beamer")],
        front: Some(7),
        ..MenuState::default()
    };
    assert_eq!(
        listed(&display_menu(&outputs)),
        [("Built-in Display".into(), false), ("Beamer".into(), true),]
    );
}

#[test]
fn checks_the_device_the_block_in_front_casts_to_instead() {
    let mut outputs = MenuState {
        displays: vec![display(1, "Built-in Display")],
        front: Some(1),
        devices: vec![tv("a", "Bedroom"), tv("b", "Living Room")],
        ..MenuState::default()
    };
    assert_eq!(
        listed(&display_menu(&outputs)),
        [
            ("Built-in Display".into(), true),
            ("Bedroom".into(), false),
            ("Living Room".into(), false),
        ]
    );
    outputs.casting = Some(tv("b", "Living Room"));
    assert_eq!(
        listed(&display_menu(&outputs)),
        [
            ("Built-in Display".into(), false),
            ("Bedroom".into(), false),
            ("Living Room".into(), true),
        ]
    );
    // Still cast to while it isn't found.
    outputs.devices.pop();
    assert_eq!(
        listed(&display_menu(&outputs)).last().cloned(),
        Some(("Living Room".into(), true))
    );
}

#[test]
fn a_device_found_by_two_protocols_shows_as_two() {
    let dlna = |id: &str, name: &str| Device {
        kind: "DLNA",
        ..speaker(id, name)
    };
    // The Phantom as an AirPlay speaker, an output, and with DLNA, a
    // speaker cast to.
    let devices = [device("desk", "Desk"), device("airplay:ph", "Phantom")];
    let outputs = MenuState {
        speakers: vec![dlna("uuid-1", "Phantom"), speaker("k", "Kitchen")],
        ..MenuState::default()
    };
    assert_eq!(
        listed(&output_menu(&devices, None, &outputs)),
        [
            ("System Default".into(), true),
            ("Desk".into(), false),
            ("Phantom (AirPlay)".into(), false),
            ("Phantom (DLNA)".into(), false),
            ("Kitchen".into(), false),
        ]
    );
    assert_eq!(
        listed(&block_output_menu(
            &[device("hdmi", "Beamer"), device("airplay:tv", "Beamer")],
            None
        )),
        [
            ("Same as the App".into(), true),
            ("Beamer".into(), false),
            ("Beamer (AirPlay)".into(), false),
        ],
        "this computer's own output keeps its name"
    );
    // A TV with two, beside a display of that name.
    let outputs = MenuState {
        displays: vec![display(1, "Living Room")],
        devices: vec![tv("a", "Living Room"), dlna("b", "Living Room")],
        ..MenuState::default()
    };
    assert_eq!(
        listed(&display_menu(&outputs)),
        [
            ("Living Room".into(), false),
            ("Living Room (Fake)".into(), false),
            ("Living Room (DLNA)".into(), false),
        ]
    );
    let outputs = MenuState {
        devices: vec![tv("a", "Living Room")],
        ..outputs
    };
    assert_eq!(
        listed(&display_menu(&outputs)),
        [
            ("Living Room".into(), false),
            ("Living Room (Fake)".into(), false),
        ],
        "told apart from the display too"
    );
}

#[test]
fn lists_the_output_picked_while_it_is_away() {
    let picked = AudioDevice {
        id: "c".into(),
        name: "TV".into(),
    };
    assert_eq!(
        listed(&output_menu(
            &[device("a", "Speakers")],
            Some(&picked),
            &MenuState::default()
        )),
        [
            ("System Default".into(), false),
            ("Speakers".into(), false),
            ("TV (Not Connected)".into(), true),
        ]
    );
}

#[test]
fn checks_the_output_the_block_in_front_plays_through() {
    let devices = [device("a", "Speakers"), device("b", "TV")];
    assert_eq!(
        listed(&block_output_menu(&devices, None)),
        [
            ("Same as the App".into(), true),
            ("Speakers".into(), false),
            ("TV".into(), false),
        ]
    );
    let own = AudioDevice {
        id: "b".into(),
        name: "TV".into(),
    };
    assert_eq!(
        listed(&block_output_menu(&devices, Some(&own))),
        [
            ("Same as the App".into(), false),
            ("Speakers".into(), false),
            ("TV".into(), true),
        ]
    );
    assert_eq!(
        listed(&block_output_menu(&devices[..1], Some(&own))),
        [
            ("Same as the App".into(), false),
            ("Speakers".into(), false),
            ("TV (Not Connected)".into(), true),
        ],
        "listed while it is away"
    );
}

#[gpui::test]
fn lists_the_devices_with_video_and_checks_the_one_cast_to(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let fake = FakeTarget::default();
    let tv = fake.add("Living Room", Shows::Video);
    fake.add("Kitchen", Shows::Sound);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![testing::fixture("clip.mp4")], cx)
        })
        .unwrap();
    let start = Instant::now();
    while !testing::shows(&mut cx, "block-0") {
        assert!(start.elapsed() < OPEN_WAIT, "clip.mp4 didn't open");
        std::thread::sleep(Duration::from_millis(5));
    }
    // The devices, after the displays.
    let menu = |cx: &mut VisualTestContext| {
        let outputs = TestAppContext::update(cx, MenuState::now);
        listed(&display_menu(&outputs))
            .into_iter()
            .skip(outputs.displays.len())
            .collect::<Vec<_>>()
    };
    assert_eq!(menu(&mut cx), [("Living Room".into(), false)]);

    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.with_active(cx, |view, cx| view.cast_to(Some(tv.clone()), cx))
        })
        .unwrap();
    assert_eq!(menu(&mut cx), [("Living Room".into(), true)]);
}

/// The window with `file` open in it, its media known, and casting
/// that has found a TV and a speaker.
fn open_with_devices(cx: &mut TestAppContext, file: &str) -> VisualTestContext {
    let (window, mut cx) = testing::start(cx);
    let fake = FakeTarget::default();
    fake.add("Living Room", Shows::Video);
    fake.add("Kitchen", Shows::Sound);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![testing::fixture(file)], cx)
        })
        .unwrap();
    let start = Instant::now();
    let known = |cx: &mut VisualTestContext| {
        let mut known = false;
        window
            .update(cx, |workspace, _, cx| {
                workspace.with_active(cx, |view, _| known = view.player().is_known())
            })
            .unwrap();
        known
    };
    while !known(&mut cx) {
        assert!(start.elapsed() < OPEN_WAIT, "{file} didn't open");
        std::thread::sleep(Duration::from_millis(5));
        cx.run_until_parked();
    }
    cx
}

#[gpui::test]
fn audio_checks_the_output_the_block_in_front_plays_through(cx: &mut TestAppContext) {
    let mut cx = open_with_devices(cx, "clip.mp4");
    let block_outputs = |cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| {
            let state = MenuState::now(cx);
            cx.set_global(state);
            let audio = menus(cx).into_iter().find(|menu| menu.name == "Audio");
            let items = audio.expect("an Audio menu").items;
            let outputs = items.into_iter().find_map(|item| match item {
                MenuItem::Submenu(menu) if menu.name == "Block Output Device" => Some(menu),
                _ => None,
            });
            checked(&outputs.expect("Block Output Device"))
        })
    };
    let devices = vec![device("desk", "Desk"), device("tv", "TV")];
    cx.update(|_, cx| AudioEngine::set_devices(devices, cx));
    assert_eq!(block_outputs(&mut cx), ["Same as the App"]);
    let pick = Invocation::new("output.set_block_device").with_args(json!({"id": "tv"}));
    TestAppContext::update(&cx, |cx| api::run(pick, cx)).unwrap();
    cx.run_until_parked();
    assert_eq!(block_outputs(&mut cx), ["TV"]);
}

#[gpui::test]
fn lists_the_speakers_for_a_block_with_sound(cx: &mut TestAppContext) {
    let cx = open_with_devices(cx, "clip.mp4");
    let outputs = TestAppContext::update(&cx, MenuState::now);
    let names = |devices: &[Device]| {
        devices
            .iter()
            .map(|device| device.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&outputs.speakers), ["Kitchen"]);
    assert_eq!(names(&outputs.devices), ["Living Room"]);
}

#[gpui::test]
fn lists_no_speakers_for_a_block_without_sound(cx: &mut TestAppContext) {
    let cx = open_with_devices(cx, "clip-ip.mkv");
    let outputs = TestAppContext::update(&cx, MenuState::now);
    assert!(outputs.speakers.is_empty(), "{:?}", outputs.speakers);
    assert_eq!(outputs.devices.len(), 1, "the TV still");
}

/// Every item in `menus`, submenus' too, with its title.
fn every_item(menus: &[Menu]) -> Vec<(String, Box<dyn gpui::Action>, bool)> {
    fn walk(items: &[MenuItem], found: &mut Vec<(String, Box<dyn gpui::Action>, bool)>) {
        for item in items {
            match item {
                MenuItem::Action {
                    name,
                    action,
                    checked,
                    ..
                } => found.push((name.to_string(), action.boxed_clone(), *checked)),
                MenuItem::Submenu(menu) => walk(&menu.items, found),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    for menu in menus {
        walk(&menu.items, &mut found);
    }
    found
}

/// The command an item runs.
fn invocation(action: &dyn gpui::Action) -> api::Invocation {
    let run = action.as_any().downcast_ref::<Run>();
    run.expect("items run commands").0.clone()
}

#[gpui::test]
fn every_item_runs_a_command_titled_as_it_is(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![testing::fixture("clip-chapters.mkv")], cx)
        })
        .unwrap();
    let known = testing::wait_until(&mut cx, |cx| {
        let state = TestAppContext::update(cx, MenuState::now);
        !state.chapters.is_empty()
    });
    assert!(known, "its chapters are known");
    let items = TestAppContext::update(&cx, |cx| {
        let state = MenuState::now(cx);
        cx.set_global(state);
        every_item(&menus(cx))
    });

    let mut commands = std::collections::HashSet::new();
    for (name, action, _) in &items {
        let invocation = invocation(&**action);
        let spec = api::find(&invocation.command)
            .unwrap_or_else(|| panic!("{name}: no command {}", invocation.command));
        api::check_args(spec, &invocation.args).unwrap_or_else(|err| panic!("{name}: {err}"));
        if spec.args.is_empty() {
            assert_eq!(name, spec.title, "{}", spec.id);
        }
        commands.insert(spec.id);
    }
    // Each command once, but those with arguments, given each.
    let takes_args = |action: &dyn gpui::Action| {
        let command = invocation(action).command;
        !api::find(&command).unwrap().args.is_empty()
    };
    let plain = items
        .iter()
        .filter(|(_, action, _)| !takes_args(&**action))
        .count();
    let with_args: std::collections::HashSet<_> = items
        .iter()
        .filter(|(_, action, _)| takes_args(&**action))
        .map(|(_, action, _)| invocation(&**action).command)
        .collect();
    assert_eq!(plain + with_args.len(), commands.len(), "one item each");
    for command in [
        "playback.go_to_chapter",
        "video.set_aspect_ratio",
        "video.set_crop",
        "output.set_device",
        "output.set_block_device",
        "window.show_on_display",
    ] {
        assert!(commands.contains(command), "no item runs {command}");
    }
}

#[gpui::test]
fn checks_what_is_on_and_enables_what_can_run(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let enabled = |command: &str, cx: &mut TestAppContext| {
        TestAppContext::update(cx, |cx| cx.is_menu_action_enabled(&Run::command(command)))
    };
    // As the menu bar was last set.
    let checked = |title: &str, cx: &mut TestAppContext| {
        let items = TestAppContext::update(cx, |cx| {
            assert!(cx.try_global::<MenuState>().is_some(), "the menus are set");
            every_item(&menus(cx))
        });
        items
            .into_iter()
            .find(|(name, _, _)| name == title)
            .is_some_and(|(_, _, checked)| checked)
    };
    // An empty window: nothing to play.
    assert!(!enabled("playback.play_pause", &mut cx));
    assert!(!enabled("block.close", &mut cx));
    assert!(enabled("window.open_files", &mut cx));
    assert!(enabled("app.quit", &mut cx));

    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![testing::fixture("clip.mp4")], cx)
        })
        .unwrap();
    let opened = testing::wait_until(&mut cx, |cx| enabled("playback.play_pause", cx));
    assert!(opened, "a file to play");
    assert!(enabled("block.close", &mut cx));
    assert_eq!(
        enabled("block.toggle_picture_in_picture", &mut cx),
        cfg!(target_os = "macos")
    );

    assert!(!checked("Loop", &mut cx));
    cx.dispatch_action(Run::command("playback.toggle_loop"));
    cx.run_until_parked();
    assert!(checked("Loop", &mut cx), "at once");
    assert!(!checked("Float on Top", &mut cx));
    cx.dispatch_action(Run::command("window.toggle_float_on_top"));
    cx.run_until_parked();
    assert!(checked("Float on Top", &mut cx));
    assert!(!checked("Float on Top While Playing", &mut cx));
    assert!(checked("Show Notes", &mut cx), "notes show at first");
    cx.dispatch_action(Run::command("block.toggle_notes"));
    cx.run_until_parked();
    assert!(!checked("Show Notes", &mut cx), "in the block in front");
    assert!(
        checked("Show Notes in Window", &mut cx),
        "but not its window"
    );
    cx.dispatch_action(Run::command("app.toggle_notes"));
    cx.run_until_parked();
    assert!(!checked("Show Notes in Every Window", &mut cx));
    assert!(!checked("Show Notes in Window", &mut cx), "nor the window");
}
