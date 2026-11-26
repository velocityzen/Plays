//! The palette, opened with its keys over a player window and driven
//! from the keyboard, as someone at the screen would (see `ui::testing`).

use gpui::{
    Bounds, Entity, Modifiers, Pixels, Rems, TestAppContext, VisualTestContext, WindowHandle,
    point, px,
};
use serde_json::{Map, json};

use super::asking::{self, Ask};
use super::{COMMAND, Palette, State, recents};
use crate::api::{self, Invocation, Source, Target};
use crate::app::player_windows;
use crate::files::Folder;
use crate::settings::Settings;
use crate::ui::testing::{self, OPEN_WAIT, fixture, press, shows, wait_until};
use crate::ui::theme::layout::{WINDOW_MIN_SIZE, WINDOW_SIZE};
use crate::ui::theme::palette::{CHECK_WIDTH, FIELD_HEIGHT, MARGIN, ROW_HEIGHT};
use crate::ui::workspace::Workspace;

/// Open the palette over the window in front with its keys.
pub(super) fn open_palette(cx: &mut VisualTestContext) {
    press(cx, "secondary-shift-p");
    cx.run_until_parked();
}

pub(super) fn palette(
    window: WindowHandle<Workspace>,
    cx: &VisualTestContext,
) -> Option<Entity<Palette>> {
    window
        .read_with(cx, |workspace, _| workspace.palette())
        .unwrap()
}

/// What `f` reads of the palette over `window`, which is up.
pub(super) fn read<R>(
    window: WindowHandle<Workspace>,
    cx: &VisualTestContext,
    f: impl FnOnce(&Palette) -> R,
) -> R {
    let palette = palette(window, cx).expect("the palette is up");
    palette.read_with(cx, |palette, _| f(palette))
}

pub(super) fn listed(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<String> {
    read(window, cx, Palette::listed)
}

/// Each heading the palette over `window` shows, and what it lists under
/// it: with no headings, all of it under "".
pub(super) fn sections(
    window: WindowHandle<Workspace>,
    cx: &VisualTestContext,
) -> Vec<(String, Vec<String>)> {
    read(window, cx, Palette::sections)
}

/// Whether `text` is drawn.
pub(super) fn painted(cx: &mut VisualTestContext, text: &str) -> bool {
    times_painted(cx, text) > 0
}

/// How many times `text` is drawn.
pub(super) fn times_painted(cx: &mut VisualTestContext, text: &str) -> usize {
    testing::painted(cx)
        .iter()
        .filter(|(drawn, _)| drawn == text)
        .count()
}

/// Whether anything tagged `selector`, made as the test goes, is drawn.
fn shows_made(cx: &mut VisualTestContext, selector: String) -> bool {
    shows(cx, Box::leak(selector.into_boxed_str()))
}

/// Type `text` into the palette's field, after what's there.
pub(super) fn type_in(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Take away what's typed in the palette's field.
pub(super) fn clear(cx: &mut VisualTestContext) {
    press(cx, "secondary-a backspace");
    cx.run_until_parked();
}

/// Press `keys` and let what they start finish.
pub(super) fn press_and_wait(cx: &mut VisualTestContext, keys: &str) {
    press(cx, keys);
    cx.run_until_parked();
}

/// The commands the palette ran since asked last: not the one that opened
/// it, or the queries it read to ask with.
pub(super) fn ran(cx: &mut VisualTestContext) -> Vec<Invocation> {
    cx.update(|_, cx| api::recorded(cx))
        .into_iter()
        .filter(|invocation| invocation.source == Source::Palette)
        .filter(|invocation| api::find(&invocation.command).is_some_and(|spec| !spec.query))
        .collect()
}

/// The API's name for `window`, as a target.
pub(super) fn aimed_at(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Target {
    Target {
        window: Some(
            window
                .read_with(cx, |workspace, _| workspace.id().to_string())
                .unwrap(),
        ),
        block: None,
    }
}

/// Open the fixtures `files` into `window`, and wait until they have.
pub(super) fn open(window: WindowHandle<Workspace>, cx: &mut VisualTestContext, files: &[&str]) {
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
    assert!(has("window.new") && has("app.settings.open") && has("window.open_url"));
    assert!(
        !has("block.playback.loop.toggle"),
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
    assert!(shows(&mut cx, "palette-row-app.about"));

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
        listed(window, &cx).contains(&"app.settings.open".to_owned()),
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
    // On its title, where someone would click it.
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
        read(window, &cx, |palette| palette
            .state("block.playback.loop.toggle")),
        Some(State::Nothing),
        "a file can loop now"
    );
    type_in(&mut cx, "loop");
    press_and_wait(&mut cx, "enter");
    let here = aimed_at(window, &cx);
    let mut toggled = Invocation::new("block.playback.loop.toggle").from(Source::Palette);
    toggled.target = here;
    assert_eq!(ran(&mut cx), [toggled]);

    open_palette(&mut cx);
    assert_eq!(
        read(window, &cx, |palette| palette
            .state("block.playback.loop.toggle")),
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
fn the_all_commands_run_from_it_aimed_at_no_window_as_they_reach_every_one(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"]);
    cx.update(|_, cx| api::record(cx));
    open_palette(&mut cx);
    type_in(&mut cx, "play in every window");
    press_and_wait(&mut cx, "enter");
    assert_eq!(
        ran(&mut cx),
        [Invocation::new("app.playback.play").from(Source::Palette)]
    );
    let playing = window
        .read_with(&cx, |workspace, cx| {
            workspace.players()[0].read(cx).is_playing()
        })
        .unwrap();
    assert!(playing, "it ran");
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
        Some(("block.playback.go_to_chapter", "number"))
    );
    assert!(
        shows(&mut cx, "palette-asking"),
        "it says which command asks"
    );
    assert_eq!(listed(window, &cx), ["Opening", "Middle", "End"]);
    assert_eq!(
        sections(window, &cx),
        [(String::new(), listed(window, &cx))],
        "choices have no headings"
    );
    assert_eq!(read(window, &cx, Palette::current), ["Opening"]);
    assert!(painted(&mut cx, "Pick"), "Return picks one");
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
    let mut went = Invocation::new("block.playback.go_to_chapter")
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
        Some(("block.audio.set_volume", "volume"))
    );
    assert!(shows(&mut cx, "palette-prompt"), "it says what to type");
    assert!(painted(&mut cx, "OK"), "Return takes what's typed");
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
    let mut set = Invocation::new("block.audio.set_volume")
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
    assert_eq!(commands[0].command, "block.playback.seek");
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
        Some(("app.settings.set", "key"))
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
        Some(("app.settings.set", "value"))
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
fn languages_are_typed_by_name_or_code_until_each_is_one(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let audio = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<Settings>().languages.audio.clone())
    };
    let ask = |cx: &mut VisualTestContext| {
        open_palette(cx);
        type_in(cx, "change setting");
        press_and_wait(cx, "enter");
        type_in(cx, "audio languages");
        press_and_wait(cx, "enter");
    };
    ask(&mut cx);
    assert_eq!(
        read(window, &cx, Palette::asking),
        Some(("app.settings.set", "value"))
    );
    assert!(shows(&mut cx, "palette-prompt"), "it says what to type");
    type_in(&mut cx, "Japanese, Elvish");
    press_and_wait(&mut cx, "enter");
    assert!(shows(&mut cx, "palette-error"), "no such language");
    assert!(palette(window, &cx).is_some(), "it asks again");
    clear(&mut cx);
    type_in(&mut cx, "Japanese, en ,, english");
    press_and_wait(&mut cx, "enter");
    assert!(palette(window, &cx).is_none());
    assert_eq!(audio(&mut cx), ["jpn", "eng"], "by code, once each");

    ask(&mut cx);
    press_and_wait(&mut cx, "enter");
    assert!(audio(&mut cx).is_empty(), "none: each file's own");
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
    const FLOAT: &str = "window.float_on_top.toggle";
    const COPY: &str = "app.copy_version_info";
    run(&mut cx, "copy version info");
    run(&mut cx, "float on top");
    open_palette(&mut cx);
    assert_eq!(listed(window, &cx)[..2], [FLOAT, COPY], "the latest first");
    let headed = sections(window, &cx);
    assert_eq!(
        headed[0],
        ("Recent".to_owned(), vec![FLOAT.to_owned(), COPY.to_owned()]),
        "under a heading of their own"
    );
    assert!(
        headed[1..]
            .iter()
            .all(|(_, ids)| !ids.contains(&FLOAT.to_owned())),
        "and not again under their group's"
    );
    // One run lately goes first, before one found by its title.
    type_in(&mut cx, "window");
    assert_eq!(listed_first_of(window, &cx, &[FLOAT, "window.new"]), FLOAT);
    assert_eq!(
        sections(window, &cx),
        [(String::new(), listed(window, &cx))],
        "found, they have no headings"
    );
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

#[gpui::test]
fn with_nothing_typed_each_group_is_under_its_heading(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for id in &listed(window, &cx) {
        let group = super::group(api::find(id).unwrap());
        match groups.last_mut() {
            Some((heading, ids)) if heading == group => ids.push(id.clone()),
            _ => groups.push((group.to_owned(), vec![id.clone()])),
        }
    }
    let headed = sections(window, &cx);
    assert_eq!(headed, groups, "each command under its group's name");
    let mut headings: Vec<&String> = headed.iter().map(|(heading, _)| heading).collect();
    let shown = headings.len();
    headings.sort();
    headings.dedup();
    assert_eq!(headings.len(), shown, "each group once: {headed:?}");
    // Down past the app's, to the windows'.
    assert_eq!(headed[1].0, "Windows");
    for _ in 0..headed[0].1.len() {
        press_and_wait(&mut cx, "down");
    }
    assert!(shows(&mut cx, "palette-heading-Windows"));
    assert_eq!(
        times_painted(&mut cx, "Windows"),
        1,
        "the heading, and not its rows again"
    );
    assert!(shows(&mut cx, "palette-search-icon"));

    type_in(&mut cx, "new window");
    assert_eq!(
        sections(window, &cx),
        [(String::new(), listed(window, &cx))]
    );
    assert!(!shows(&mut cx, "palette-heading-Windows"));
    assert!(
        painted(&mut cx, "Windows"),
        "found, a row says its group after its name"
    );
}

#[gpui::test]
fn the_outputs_are_audio_and_a_window_goes_back_to_the_apps_as_the_menus_say(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"]);
    open_palette(&mut cx);
    let headed = sections(window, &cx);
    let under = |heading: &str| -> Vec<&String> {
        let found = headed.iter().filter(|(found, _)| found == heading);
        found.flat_map(|(_, ids)| ids).collect()
    };
    let outputs = under("Audio Output");
    for id in ["output.set_device", "output.set_block_device"] {
        assert!(outputs.contains(&&id.to_owned()), "{id}: {headed:?}");
    }
    assert!(under("Output").is_empty(), "not just Output: {headed:?}");

    type_in(&mut cx, "block output device");
    press_and_wait(&mut cx, "enter");
    assert_eq!(
        read(window, &cx, Palette::asking),
        Some(("output.set_block_device", "id"))
    );
    assert_eq!(
        listed(window, &cx).first().map(String::as_str),
        Some("Same as the App"),
        "as Audio → Block Output Device and the block's menu say"
    );
    assert_eq!(read(window, &cx, Palette::current), ["Same as the App"]);
}

#[gpui::test]
fn pointing_at_a_row_picks_it_and_the_arrows_go_on_from_there(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    type_in(&mut cx, "copy");
    let listed = listed(window, &cx);
    let debug = listed
        .iter()
        .position(|id| id == "app.copy_debug_info")
        .expect("found");
    assert_ne!(debug, 0, "not the one picked already: {listed:?}");
    testing::hover(&mut cx, "palette-row-app.copy_debug_info");
    assert_eq!(
        read(window, &cx, Palette::picked).as_deref(),
        Some("app.copy_debug_info"),
        "what's pointed at is what Return runs"
    );
    press_and_wait(&mut cx, "up");
    assert_eq!(
        read(window, &cx, Palette::picked),
        listed.get(debug - 1).cloned(),
        "the mouse not moving, the arrows go on from it"
    );
}

#[gpui::test]
fn the_footer_says_what_return_and_command_return_do_and_each_can_be_clicked(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    cx.update(|_, cx| api::record(cx));
    open_palette(&mut cx);
    type_in(&mut cx, "change setting");
    let enter = crate::hotkeys::display("enter");
    assert!(painted(&mut cx, "Run") && painted(&mut cx, &enter));
    assert!(
        !painted(&mut cx, "Change Shortcut"),
        "a command that takes no keys"
    );
    clear(&mut cx);
    type_in(&mut cx, "zzz");
    assert!(!painted(&mut cx, "Run"), "nothing to run");
    clear(&mut cx);
    type_in(&mut cx, "new window");
    let change = crate::hotkeys::display("secondary-enter");
    assert!(painted(&mut cx, "Change Shortcut") && painted(&mut cx, &change));
    testing::click(&mut cx, "palette-return");
    cx.run_until_parked();
    assert!(palette(window, &cx).is_none());
    assert_eq!(
        ran(&mut cx),
        [Invocation::new("window.new").from(Source::Palette)]
    );

    window
        .update(&mut cx, |_, window, _| window.activate_window())
        .unwrap();
    open_palette(&mut cx);
    type_in(&mut cx, "new window");
    testing::click(&mut cx, "palette-change-shortcut");
    cx.run_until_parked();
    assert_eq!(
        read(window, &cx, Palette::changing),
        Some("window.new"),
        "its keys listed, in the palette"
    );
}

#[gpui::test]
fn the_row_picked_and_the_heading_over_it_stay_in_view(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"]);
    open_palette(&mut cx);
    let listed = listed(window, &cx);
    let headed = sections(window, &cx);
    assert!(
        listed.len() + headed.len() > crate::ui::theme::palette::ROWS,
        "more than show at once"
    );
    for (at, id) in listed.iter().enumerate() {
        if at > 0 {
            press_and_wait(&mut cx, "down");
        }
        assert!(
            shows_made(&mut cx, format!("palette-row-{id}")),
            "{id}, going down"
        );
    }
    press_and_wait(&mut cx, "down");
    assert_eq!(read(window, &cx, Palette::picked), listed.first().cloned());
    assert!(
        shows_made(&mut cx, format!("palette-heading-{}", headed[0].0)),
        "back at the first, its heading too"
    );
}

#[gpui::test]
fn in_a_short_window_it_ends_short_of_the_bottom_and_its_list_scrolls(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    cx.simulate_resize(WINDOW_MIN_SIZE);
    open_palette(&mut cx);
    // The footer is the card's last, inside its hairline border.
    let footer = testing::find(&mut cx, "palette-footer");
    let bottom = WINDOW_MIN_SIZE.height - MARGIN;
    assert!(
        footer.bottom() <= bottom && footer.bottom() > bottom - px(2.),
        "as tall as fits, a margin above the window's bottom: {footer:?}"
    );
    let field = testing::find(&mut cx, "palette-field");
    assert_near(field.size.height, FIELD_HEIGHT, "the field whole");

    // Up from the first, the last is picked, in view above the footer.
    press_and_wait(&mut cx, "up");
    let last = listed(window, &cx).last().unwrap().clone();
    assert_eq!(read(window, &cx, Palette::picked), Some(last.clone()));
    let row = testing::find(&mut cx, Box::leak(format!("palette-row-{last}").into()));
    assert!(
        row.top() >= field.bottom() && row.bottom() <= footer.top(),
        "{row:?} between {field:?} and {footer:?}"
    );

    cx.simulate_resize(WINDOW_SIZE);
    let footer = testing::find(&mut cx, "palette-footer");
    assert!(
        footer.bottom() < WINDOW_SIZE.height - MARGIN - ROW_HEIGHT,
        "in a tall window, only as tall as its rows: {footer:?}"
    );
}

/// Where `text` is drawn in the element tagged `selector`.
fn painted_in(cx: &mut VisualTestContext, text: &str, selector: String) -> Bounds<Pixels> {
    let within = testing::find(cx, Box::leak(selector.into_boxed_str()));
    testing::painted(cx)
        .into_iter()
        .find(|(drawn, at)| drawn == text && within.contains(&at.center()))
        .unwrap_or_else(|| panic!("{text:?} isn't drawn in {within:?}"))
        .1
}

/// How tall a line of text `size` big is in the window.
fn line_height(cx: &mut VisualTestContext, size: Rems) -> Pixels {
    cx.update(|window, _| {
        let mut style = window.text_style();
        style.font_size = size.into();
        style.line_height_in_pixels(window.rem_size())
    })
}

/// The text of a menu row, a text field or a setting (`text_sm`).
const TEXT: Rems = Rems(0.875);
/// The text of a heading in a menu, or of a hint (`text_xs`).
const SMALL: Rems = Rems(0.75);

fn assert_near(a: Pixels, b: Pixels, what: &str) {
    assert!((a - b).abs() < px(0.5), "{what}: {a:?}, not {b:?}");
}

#[gpui::test]
fn rows_fill_the_list_as_wide_as_the_field_and_end_in_their_keys(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    let field = testing::find(&mut cx, "palette-field");
    let (heading, ids) = sections(window, &cx).remove(0);
    let first = format!("palette-row-{}", ids[0]);
    let row = testing::find(&mut cx, Box::leak(first.clone().into_boxed_str()));
    assert_near(
        row.left(),
        field.left(),
        "a row starts where the field does",
    );
    assert_near(row.right(), field.right(), "and ends where it does");
    let title = api::find(&ids[0]).unwrap().title;
    assert_near(
        painted_in(&mut cx, &heading, format!("palette-heading-{heading}")).left(),
        painted_in(&mut cx, title, first).left(),
        "a heading starts where its rows' names do",
    );

    type_in(&mut cx, "new window");
    let picked = "palette-row-window.new";
    let row = testing::find(&mut cx, picked);
    assert_near(
        row.left(),
        field.left(),
        "the row picked starts where the field does",
    );
    assert_near(row.right(), field.right(), "and ends where it does");
    let keys = crate::hotkeys::display("secondary-n");
    let keys = painted_in(&mut cx, &keys, picked.into());
    let name = painted_in(&mut cx, "New Window", picked.into());
    assert!(
        keys.left() > row.center().x,
        "its keys at its end: {keys:?} in {row:?}"
    );
    assert_near(
        row.right() - keys.right(),
        name.left() - row.left() - CHECK_WIDTH - px(8.),
        "as far from its end as its name, the check aside, from its start",
    );
}

#[gpui::test]
fn its_text_is_as_big_as_in_menus_and_settings(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    let (heading, _) = sections(window, &cx).remove(0);
    let (text, small) = (line_height(&mut cx, TEXT), line_height(&mut cx, SMALL));
    assert!(small < text);
    let drawn = painted_in(&mut cx, &heading, format!("palette-heading-{heading}"));
    assert_near(drawn.size.height, small, "a heading");

    type_in(&mut cx, "new window");
    let row = "palette-row-window.new";
    let drawn = painted_in(&mut cx, "New Window", row.into());
    assert_near(drawn.size.height, text, "a row");
    let keys = crate::hotkeys::display("secondary-n");
    let drawn = painted_in(&mut cx, &keys, row.into());
    assert_near(drawn.size.height, text, "its keys");
    let drawn = painted_in(&mut cx, "Run", "palette-return".into());
    assert_near(drawn.size.height, small, "the footer");
    let enter = crate::hotkeys::display("enter");
    let drawn = painted_in(&mut cx, &enter, "palette-return".into());
    assert_near(drawn.size.height, small, "its keycaps");

    clear(&mut cx);
    type_in(&mut cx, "zzz");
    let drawn = painted_in(&mut cx, "No commands found", "palette-empty".into());
    assert_near(drawn.size.height, text, "what it says with nothing found");

    clear(&mut cx);
    type_in(&mut cx, "new window");
    press_and_wait(&mut cx, "secondary-enter");
    let press = "Press a shortcut for New Window";
    let drawn = painted_in(&mut cx, press, "palette-press".into());
    assert_near(drawn.size.height, text, "what's in the field");
    let drawn = painted_in(&mut cx, "New Window", "palette-asking".into());
    assert_near(drawn.size.height, small, "the command's chip in it");
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
fn a_command_that_takes_no_keys_has_no_shortcut_to_change(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    type_in(&mut cx, "change setting");
    assert_eq!(
        read(window, &cx, Palette::picked).as_deref(),
        Some("app.settings.set")
    );
    assert!(!shows(&mut cx, "palette-change-shortcut"));
    press_and_wait(&mut cx, "secondary-enter");
    assert!(palette(window, &cx).is_some(), "it stays up");
    assert_eq!(read(window, &cx, Palette::changing), None);
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
    assert_eq!(
        typed,
        [
            "app.open",
            "block.playback.seek",
            "window.playback.seek",
            "app.playback.seek",
            "block.audio.set_volume",
            "window.audio.set_volume",
            "app.audio.set_volume",
        ]
    );
    let unasked: Vec<&str> = asks
        .iter()
        .filter(|(_, how)| *how == "nothing")
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(
        unasked,
        ["window.rename", "block.rename"],
        "these ask for a name themselves"
    );
    let open = api::find("app.open").unwrap();
    let urls = serde_json::from_value(json!({"urls": ["/a.mkv"]})).unwrap();
    let then = TestAppContext::update(&cx, |cx| asking::next(open, &["urls"], &urls, &here, cx));
    assert!(then.is_none(), "it opens as Settings say");
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
        choices("block.subtitles.select_track", &mut cx).len() > 1
    });
    assert!(known, "its tracks are known");
    let subtitles = choices("block.subtitles.select_track", &mut cx);
    assert_eq!(labels(&subtitles), ["Off", "English"]);
    assert_eq!(subtitles[0].args, Map::new(), "off has no number");
    assert_eq!(
        subtitles[1].args,
        json!({"number": 1}).as_object().unwrap().clone()
    );
    assert_eq!(subtitles.iter().filter(|choice| choice.current).count(), 1);
    let audio = choices("block.audio.select_track", &mut cx);
    assert_eq!(audio.len(), 1);
    assert!(audio[0].current, "the track playing");

    let looked = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace.players()[0].read(cx).knows_neighbors()
            })
            .unwrap()
    });
    assert!(looked, "its folder is looked through");
    let files = choices("block.playback.play_file", &mut cx);
    let opus = files
        .iter()
        .find(|choice| choice.label.as_ref() == "clip-opus.mkv")
        .expect("the files in its folder");
    assert_eq!(opus.args["path"], json!(fixture("clip-opus.mkv")));
    let playing: Vec<String> = labels(&files)
        .into_iter()
        .zip(&files)
        .filter(|(_, choice)| choice.current)
        .map(|(label, _)| label)
        .collect();
    assert_eq!(playing, ["clip-subs.mkv"], "the one it plays");

    let ratios = choices("block.video.set_aspect_ratio", &mut cx);
    assert_eq!(ratios[0].label.as_ref(), "Original");
    assert!(ratios[0].current, "none set");
    assert!(labels(&ratios).contains(&"16:9".to_owned()));

    let panels = choices("block.set_ui", &mut cx);
    assert_eq!(
        labels(&panels),
        [
            "None",
            "All",
            "Only Controls",
            "Only Top Bar",
            "Only Track Switcher",
            "Only Notes"
        ]
    );
    assert_eq!(panels[0].args["visible"], json!([]));
    assert!(panels[1].current, "each shows");

    let windows = choices("block.move_to_window", &mut cx);
    assert!(windows.is_empty(), "no other window to move to");
    let places = choices("window.move", &mut cx);
    assert_eq!(labels(&places), ["1"]);
    assert!(places[0].current);

    let outputs = choices("output.set_device", &mut cx);
    assert_eq!(outputs[0].label.as_ref(), "System Default");
    let keys = choices("app.settings.reset", &mut cx);
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

/// Moving the cursor in the field, or selecting in it, keeps the row picked.
#[gpui::test]
fn moving_the_cursor_keeps_the_row_picked(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open_palette(&mut cx);
    type_in(&mut cx, "window");
    let rows = listed(window, &cx);
    assert!(rows.len() > 1, "{rows:?}");
    press_and_wait(&mut cx, "down");
    assert_eq!(read(window, &cx, Palette::picked), rows.get(1).cloned());
    press_and_wait(&mut cx, "left left right home end shift-left");
    assert_eq!(read(window, &cx, Palette::picked), rows.get(1).cloned());
    assert_eq!(listed(window, &cx), rows);
}
