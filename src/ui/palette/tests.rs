//! The palette, opened with its keys over a player window and driven
//! from the keyboard, as someone at the screen would (see `ui::testing`).

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, WindowHandle, point, px};
use serde_json::{Map, json};

use super::asking::{self, Ask};
use super::{COMMAND, Palette, State, recents};
use crate::api::{self, Invocation, Source, Target};
use crate::app::player_windows;
use crate::files::Folder;
use crate::settings::Settings;
use crate::ui::settings::SettingsView;
use crate::ui::testing::{self, OPEN_WAIT, fixture, press, shows, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// Open the palette over the window in front with its keys.
fn open_palette(cx: &mut VisualTestContext) {
    press(cx, "secondary-shift-p");
    cx.run_until_parked();
}

fn palette(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Option<Entity<Palette>> {
    window
        .read_with(cx, |workspace, _| workspace.palette())
        .unwrap()
}

/// What `f` reads of the palette over `window`, which is up.
fn read<R>(
    window: WindowHandle<Workspace>,
    cx: &VisualTestContext,
    f: impl FnOnce(&Palette) -> R,
) -> R {
    let palette = palette(window, cx).expect("the palette is up");
    palette.read_with(cx, |palette, _| f(palette))
}

fn listed(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<String> {
    read(window, cx, Palette::listed)
}

/// Type `text` into the palette's field, after what's there.
fn type_in(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Take away what's typed in the palette's field.
fn clear(cx: &mut VisualTestContext) {
    press(cx, "secondary-a backspace");
    cx.run_until_parked();
}

/// Press `keys` and let what they start finish.
fn press_and_wait(cx: &mut VisualTestContext, keys: &str) {
    press(cx, keys);
    cx.run_until_parked();
}

/// The commands the palette ran since asked last: not the one that opened
/// it, or the queries it read to ask with.
fn ran(cx: &mut VisualTestContext) -> Vec<Invocation> {
    cx.update(|_, cx| api::recorded(cx))
        .into_iter()
        .filter(|invocation| invocation.source == Source::Palette)
        .filter(|invocation| api::find(&invocation.command).is_some_and(|spec| !spec.query))
        .collect()
}

/// The API's name for `window`, as a target.
fn aimed_at(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Target {
    Target {
        window: Some(
            window
                .read_with(cx, |workspace, _| workspace.id().to_string())
                .unwrap(),
        ),
        tile: None,
    }
}

/// Open the fixtures `files` into `window`, and wait until they have.
fn open(window: WindowHandle<Workspace>, cx: &mut VisualTestContext, files: &[&str]) {
    let paths = files.iter().map(|file| fixture(file)).collect();
    window
        .update(cx, |workspace, _, cx| workspace.open_paths(paths, cx))
        .unwrap();
    let opened = wait_until(cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                let players = workspace.players();
                players.len() == files.len()
                    && players
                        .iter()
                        .all(|player| player.read(cx).player().is_known())
            })
            .unwrap()
    });
    assert!(opened, "{files:?} didn't open in {OPEN_WAIT:?}");
}

#[gpui::test]
fn its_keys_list_every_command_that_can_run_in_the_window(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    assert!(shows(&mut cx, "palette"));
    let listed = listed(window, &cx);
    let has = |id: &str| listed.iter().any(|listed| listed == id);
    assert!(has("window.new") && has("app.settings") && has("window.open_url"));
    assert!(
        !has("playback.toggle_loop"),
        "an empty window has no file to loop"
    );
    assert!(!has("window.ls"), "a query only reads");
    assert!(!has("window.focus"), "it must be told which window");
    assert!(!has(COMMAND), "not its own");
    // With nothing typed or run lately, as the commands are listed.
    let order: Vec<usize> = listed
        .iter()
        .map(|id| api::all().position(|spec| spec.id == id).unwrap())
        .collect();
    assert!(order.is_sorted(), "{listed:?}");
    assert_eq!(read(window, &cx, Palette::picked), listed.first().cloned());
    assert!(shows(&mut cx, "palette-row-app.settings"));

    press_and_wait(&mut cx, "escape");
    assert!(palette(window, &cx).is_none(), "Escape closes it");
    open_palette(&mut cx);
    let corner = point(px(20.), WINDOW_SIZE.height - px(20.));
    cx.simulate_click(corner, Modifiers::none());
    cx.run_until_parked();
    assert!(
        palette(window, &cx).is_none(),
        "and so does a click outside"
    );
}

#[gpui::test]
fn typing_finds_commands_by_their_titles_groups_and_keywords(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    type_in(&mut cx, "new win");
    assert_eq!(
        listed(window, &cx).first().map(String::as_str),
        Some("window.new")
    );
    clear(&mut cx);
    type_in(&mut cx, "preferences");
    assert!(
        listed(window, &cx).contains(&"app.settings".to_owned()),
        "a keyword: {:?}",
        listed(window, &cx)
    );
    clear(&mut cx);
    type_in(&mut cx, "windows new");
    assert_eq!(
        listed(window, &cx).first().map(String::as_str),
        Some("window.new"),
        "its group, Windows, in any order"
    );
    type_in(&mut cx, "zzz");
    assert!(listed(window, &cx).is_empty());
    assert!(shows(&mut cx, "palette-empty"), "it says none are found");
}

#[gpui::test]
fn the_arrows_pick_a_row_and_return_runs_its_command_from_the_palette(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    cx.update(|_, cx| api::record(cx));
    open_palette(&mut cx);
    let listed = listed(window, &cx);
    press_and_wait(&mut cx, "down down");
    assert_eq!(read(window, &cx, Palette::picked), listed.get(2).cloned());
    press_and_wait(&mut cx, "up up up");
    assert_eq!(
        read(window, &cx, Palette::picked),
        listed.last().cloned(),
        "up from the first goes round to the last"
    );
    press_and_wait(&mut cx, "down");
    assert_eq!(
        read(window, &cx, Palette::picked),
        listed.first().cloned(),
        "and down from the last to the first"
    );

    type_in(&mut cx, "new window");
    press_and_wait(&mut cx, "enter");
    assert!(palette(window, &cx).is_none(), "it closes");
    assert_eq!(
        ran(&mut cx),
        [Invocation::new("window.new").from(Source::Palette)],
        "the app's command, aimed at nothing"
    );
    assert_eq!(cx.update(|_, cx| player_windows(cx).len()), 2);
}

#[gpui::test]
fn a_click_runs_a_row(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    cx.update(|_, cx| api::record(cx));
    open_palette(&mut cx);
    type_in(&mut cx, "new window");
    // Its title, not the Change Shortcut… button the row picked has.
    let row = testing::find(&mut cx, "palette-row-window.new");
    let title = point(row.origin.x + px(40.), row.center().y);
    cx.simulate_mouse_move(title, None, Modifiers::none());
    cx.simulate_click(title, Modifiers::none());
    cx.run_until_parked();
    assert!(palette(window, &cx).is_none());
    assert_eq!(
        ran(&mut cx),
        [Invocation::new("window.new").from(Source::Palette)]
    );
}

#[gpui::test]
fn rows_show_what_commands_change_and_run_on_the_window_its_over(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-chapters.mkv"]);
    cx.update(|_, cx| api::record(cx));
    open_palette(&mut cx);
    assert_eq!(
        read(window, &cx, |palette| palette.state("playback.toggle_loop")),
        Some(State::Nothing),
        "a file can loop now"
    );
    type_in(&mut cx, "loop");
    press_and_wait(&mut cx, "enter");
    let here = aimed_at(window, &cx);
    let mut toggled = Invocation::new("playback.toggle_loop").from(Source::Palette);
    toggled.target = here;
    assert_eq!(ran(&mut cx), [toggled]);

    open_palette(&mut cx);
    assert_eq!(
        read(window, &cx, |palette| palette.state("playback.toggle_loop")),
        Some(State::On),
        "a check"
    );
    assert_eq!(
        read(window, &cx, |palette| palette.state("window.move")),
        Some(State::Is("1".into())),
        "what it's set to"
    );
}

#[gpui::test]
fn a_command_taking_a_choice_asks_for_it_from_a_list(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-chapters.mkv"]);
    cx.update(|_, cx| api::record(cx));
    open_palette(&mut cx);
    type_in(&mut cx, "go to chapter");
    press_and_wait(&mut cx, "enter");
    assert_eq!(
        read(window, &cx, Palette::asking),
        Some(("playback.go_to_chapter", "number"))
    );
    assert!(
        shows(&mut cx, "palette-asking"),
        "it says which command asks"
    );
    assert_eq!(listed(window, &cx), ["Opening", "Middle", "End"]);
    assert_eq!(read(window, &cx, Palette::current), ["Opening"]);
    assert_eq!(
        read(window, &cx, Palette::picked).as_deref(),
        Some("Opening"),
        "the one it's at is picked"
    );
    assert!(ran(&mut cx).is_empty(), "nothing runs yet");

    type_in(&mut cx, "end");
    assert_eq!(listed(window, &cx), ["End"]);
    press_and_wait(&mut cx, "enter");
    assert!(palette(window, &cx).is_none());
    let mut went = Invocation::new("playback.go_to_chapter")
        .with_args(json!({"number": 3}))
        .from(Source::Palette);
    went.target = aimed_at(window, &cx);
    assert_eq!(ran(&mut cx), [went]);
    let position = window
        .read_with(&cx, |workspace, cx| {
            workspace.players()[0].read(cx).position()
        })
        .unwrap();
    assert!((position - 1.4).abs() < 1e-6, "at {position}");
}

#[gpui::test]
fn a_command_taking_a_value_asks_for_it_typed_until_it_will_do(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"]);
    cx.update(|_, cx| api::record(cx));
    open_palette(&mut cx);
    type_in(&mut cx, "set volume");
    press_and_wait(&mut cx, "enter");
    assert_eq!(
        read(window, &cx, Palette::asking),
        Some(("audio.set_volume", "volume"))
    );
    assert!(shows(&mut cx, "palette-prompt"), "it says what to type");
    type_in(&mut cx, "150");
    press_and_wait(&mut cx, "enter");
    assert!(shows(&mut cx, "palette-error"), "too loud");
    assert!(palette(window, &cx).is_some(), "it asks again");
    clear(&mut cx);
    assert!(
        !shows(&mut cx, "palette-error"),
        "typing takes the error away"
    );
    type_in(&mut cx, "40");
    press_and_wait(&mut cx, "enter");
    assert!(palette(window, &cx).is_none());
    let mut set = Invocation::new("audio.set_volume")
        .with_args(json!({"volume": 40.0}))
        .from(Source::Palette);
    set.target = aimed_at(window, &cx);
    assert_eq!(ran(&mut cx), [set]);

    // A time, as a clock shows it.
    open_palette(&mut cx);
    type_in(&mut cx, "seek to");
    press_and_wait(&mut cx, "enter");
    type_in(&mut cx, "0:01.5");
    press_and_wait(&mut cx, "enter");
    let commands = ran(&mut cx);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].command, "playback.seek");
    assert_eq!(commands[0].args, json!({"time": 1.5}));
}

#[gpui::test]
fn a_setting_is_picked_then_its_value_as_settings_offers_them(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let shown = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<Settings>().show_titles);
    // Off, so what it is now isn't the first value offered.
    cx.update(|_, cx| Settings::update(cx, false, |s| s.show_titles = false));
    open_palette(&mut cx);
    type_in(&mut cx, "change setting");
    press_and_wait(&mut cx, "enter");
    assert_eq!(
        read(window, &cx, Palette::asking),
        Some(("settings.set", "key"))
    );
    assert!(
        !listed(window, &cx).contains(&"Shortcuts".to_owned()),
        "shortcuts aren't typed here"
    );
    type_in(&mut cx, "show file names");
    assert_eq!(
        listed(window, &cx).first().map(String::as_str),
        Some("Show file names")
    );
    press_and_wait(&mut cx, "enter");
    assert_eq!(
        read(window, &cx, Palette::asking),
        Some(("settings.set", "value"))
    );
    assert_eq!(listed(window, &cx), ["On", "Off"]);
    assert_eq!(read(window, &cx, Palette::current), ["Off"]);
    assert_eq!(
        read(window, &cx, Palette::picked).as_deref(),
        Some("Off"),
        "what it is now is picked"
    );
    type_in(&mut cx, "on");
    press_and_wait(&mut cx, "enter");
    assert!(palette(window, &cx).is_none());
    assert!(shown(&mut cx));
}

#[gpui::test]
fn commands_run_from_it_lately_come_first_and_are_kept(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let folder = Folder::for_test("palette-recents");
    cx.update(|_, cx| cx.set_global(folder.clone()));
    let run = |cx: &mut VisualTestContext, typed: &str| {
        open_palette(cx);
        type_in(cx, typed);
        press_and_wait(cx, "enter");
    };
    const FLOAT: &str = "window.toggle_float_on_top";
    const COPY: &str = "app.copy_version_info";
    run(&mut cx, "copy version info");
    run(&mut cx, "float on top");
    open_palette(&mut cx);
    assert_eq!(listed(window, &cx)[..2], [FLOAT, COPY], "the latest first");
    // One run lately goes first, before one found by its title.
    type_in(&mut cx, "window");
    assert_eq!(listed_first_of(window, &cx, &[FLOAT, "window.new"]), FLOAT);
    press_and_wait(&mut cx, "escape");

    // The next launch reads them from their file.
    cx.update(|_, cx| recents::forget(cx));
    let kept = std::fs::read_to_string(folder.file("recent-commands.json").unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&kept).unwrap(),
        [FLOAT, COPY]
    );
    open_palette(&mut cx);
    assert_eq!(listed(window, &cx)[..2], [FLOAT, COPY]);
}

/// Which of `ids` it lists first.
fn listed_first_of(
    window: WindowHandle<Workspace>,
    cx: &VisualTestContext,
    ids: &[&str],
) -> String {
    listed(window, cx)
        .into_iter()
        .find(|listed| ids.contains(&listed.as_str()))
        .expect("one of them is listed")
}

#[gpui::test]
fn change_shortcut_records_one_in_settings(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    type_in(&mut cx, "new window");
    assert!(
        shows(&mut cx, "palette-change-shortcut"),
        "on the row picked"
    );
    press_and_wait(&mut cx, "secondary-enter");
    assert!(palette(window, &cx).is_none());
    let handle = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<SettingsView>())
        .expect("Settings opens");
    let mut settings = VisualTestContext::from_window(handle.into(), &cx);
    settings.update(|window, _| window.activate_window());
    settings.run_until_parked();
    assert!(
        shows(&mut settings, "hotkey-recording-window.new"),
        "waiting for keys"
    );
    press(&mut settings, "secondary-shift-y");
    settings.run_until_parked();
    let keys = settings.update(|_, cx| cx.global::<Settings>().hotkeys.keys("window.new").to_vec());
    let key = crate::hotkeys::normalize("secondary-shift-y").unwrap();
    assert!(keys.contains(&key), "{keys:?}");
}

#[gpui::test]
fn a_command_that_takes_no_keys_has_no_shortcut_to_change(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    type_in(&mut cx, "change setting");
    assert_eq!(
        read(window, &cx, Palette::picked).as_deref(),
        Some("settings.set")
    );
    assert!(!shows(&mut cx, "palette-change-shortcut"));
    press_and_wait(&mut cx, "secondary-enter");
    assert!(palette(window, &cx).is_some(), "it stays up");
}

/// How the palette first asks for each command's arguments, from the
/// window `here`: from a list, typed, or not at all.
fn first_asks(here: &Target, cx: &mut gpui::App) -> Vec<(&'static str, &'static str)> {
    api::all()
        .filter(|spec| !spec.args.is_empty() && !spec.query)
        .map(|spec| {
            let how = match asking::next(spec, &[], &Map::new(), here, cx).map(|q| q.ask) {
                Some(Ask::Choose(_)) => "list",
                Some(Ask::Type(_)) => "typed",
                None => "nothing",
            };
            (spec.id, how)
        })
        .collect()
}

#[gpui::test]
fn every_argument_is_picked_from_a_list_but_times_volumes_paths_and_names(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-chapters.mkv"]);
    let here = aimed_at(window, &cx);
    let asks = TestAppContext::update(&cx, |cx| first_asks(&here, cx));
    let typed: Vec<&str> = asks
        .iter()
        .filter(|(_, how)| *how == "typed")
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(typed, ["app.open", "playback.seek", "audio.set_volume"]);
    let unasked: Vec<&str> = asks
        .iter()
        .filter(|(_, how)| *how == "nothing")
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(
        unasked,
        ["window.rename", "tile.rename"],
        "these ask for a name themselves"
    );
}

#[gpui::test]
fn lists_offer_what_there_is_with_what_is_picked_now(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-subs.mkv"]);
    let here = aimed_at(window, &cx);
    let choices = |command: &str, cx: &mut VisualTestContext| {
        let spec = api::find(command).unwrap();
        // Outside the window's update, as the palette asks.
        TestAppContext::update(cx, |cx| {
            match asking::next(spec, &[], &Map::new(), &here, cx).map(|q| q.ask) {
                Some(Ask::Choose(choices)) => choices,
                _ => panic!("{command} asks from a list"),
            }
        })
    };
    let labels = |choices: &[asking::Choice]| -> Vec<String> {
        choices
            .iter()
            .map(|choice| choice.label.to_string())
            .collect()
    };

    // Its tracks are known a little after it opens.
    let known = wait_until(&mut cx, |cx| {
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        choices("subtitles.select_track", &mut cx).len() > 1
    });
    assert!(known, "its tracks are known");
    let subtitles = choices("subtitles.select_track", &mut cx);
    assert_eq!(labels(&subtitles), ["Off", "English"]);
    assert_eq!(subtitles[0].args, Map::new(), "off has no number");
    assert_eq!(
        subtitles[1].args,
        json!({"number": 1}).as_object().unwrap().clone()
    );
    assert_eq!(subtitles.iter().filter(|choice| choice.current).count(), 1);
    let audio = choices("audio.select_track", &mut cx);
    assert_eq!(audio.len(), 1);
    assert!(audio[0].current, "the track playing");

    let ratios = choices("video.set_aspect_ratio", &mut cx);
    assert_eq!(ratios[0].label.as_ref(), "Original");
    assert!(ratios[0].current, "none set");
    assert!(labels(&ratios).contains(&"16:9".to_owned()));

    let windows = choices("tile.move_to_window", &mut cx);
    assert!(windows.is_empty(), "no other window to move to");
    let places = choices("window.move", &mut cx);
    assert_eq!(labels(&places), ["1"]);
    assert!(places[0].current);

    let outputs = choices("output.set_device", &mut cx);
    assert_eq!(outputs[0].label.as_ref(), "System Default");
    let keys = choices("settings.reset", &mut cx);
    assert!(
        labels(&keys).contains(&"Shortcuts".to_owned()),
        "they can be put back"
    );
}

fn choice(label: &str, detail: Option<&str>) -> asking::Choice {
    asking::Choice {
        label: label.to_owned().into(),
        detail: detail.map(|detail| detail.to_owned().into()),
        args: Map::new(),
        current: false,
    }
}

#[test]
fn choices_are_found_by_name_or_detail_best_first() {
    let choices = [
        choice("Track Two", Some("0:00")),
        choice("Two", Some("1:30")),
        choice("Three", None),
    ];
    let indices = |query: &str| -> Vec<usize> {
        super::order_choices(query, &choices)
            .into_iter()
            .map(|row| row.index)
            .collect()
    };
    assert_eq!(
        indices(""),
        [0, 1, 2],
        "all, as they are, with nothing typed"
    );
    assert_eq!(indices("two"), [1, 0], "side by side first");
    assert_eq!(indices("1:30"), [1], "by its detail");
    assert_eq!(
        super::order_choices("two", &choices)[0].at,
        [0, 1, 2],
        "the letters of its name found"
    );
    assert!(
        super::order_choices("1:30", &choices)[0].at.is_empty(),
        "none in its name"
    );
}

#[gpui::test]
fn it_keeps_the_latest_few_once_each(cx: &mut TestAppContext) {
    cx.update(|cx| {
        for n in 0..10 {
            recents::note(&format!("test.c{n}"), cx);
        }
        let kept: Vec<String> = (2..10).rev().map(|n| format!("test.c{n}")).collect();
        assert_eq!(recents::list(cx), kept);
        assert_eq!(kept.len(), recents::KEPT);
        recents::note("test.c5", cx);
        let listed = recents::list(cx);
        assert_eq!(listed[0], "test.c5");
        assert_eq!(
            listed.iter().filter(|id| *id == "test.c5").count(),
            1,
            "once"
        );
        assert_eq!(listed.len(), recents::KEPT);
    });
}
