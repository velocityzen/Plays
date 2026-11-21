//! The registry, and commands run through `run` as the command line and
//! palette are to run them: aimed at a window or tile by name, or at the
//! window in front, and turned away as they should be.

use std::collections::HashSet;

use gpui::{AnyWindowHandle, Entity, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::json;

use super::registry::{self, Scope};
use super::*;
use crate::hotkeys::Command;
use crate::ui::player::PlayerView;
use crate::ui::settings::SettingsView;
use crate::ui::testing::{self, fixture};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// The namespaces command ids start with.
const NAMESPACES: [&str; 10] = [
    "app",
    "window",
    "tile",
    "playback",
    "audio",
    "subtitles",
    "video",
    "cast",
    "output",
    "settings",
];

#[test]
fn every_command_has_an_id_of_its_own_a_title_and_a_description() {
    let mut ids = HashSet::new();
    let mut titles = HashSet::new();
    for spec in registry::all() {
        let id = spec.id;
        assert!(ids.insert(id), "{id} twice");
        assert!(titles.insert(spec.title), "{} twice", spec.title);
        assert!(NAMESPACES.contains(&spec.namespace()), "{id}'s namespace");
        let name = &id[spec.namespace().len() + 1..];
        assert!(
            name.split('.')
                .all(|part| !part.is_empty()
                    && part.chars().all(|c| c.is_ascii_lowercase() || c == '_')),
            "{id} is lowercase words"
        );
        assert!(!spec.title.is_empty(), "{id}'s title");
        assert!(
            spec.description.ends_with('.') && spec.description.len() > 12,
            "{id} says what it does"
        );
        assert!(
            spec.keywords
                .iter()
                .all(|word| *word == word.to_lowercase()),
            "{id}'s keywords"
        );
        let names: HashSet<&str> = spec.args.iter().map(|arg| arg.name).collect();
        assert_eq!(names.len(), spec.args.len(), "{id}'s arguments");
        assert!(
            spec.hotkey.is_none() || spec.keys.is_empty(),
            "{id}'s keys are its shortcut's"
        );
        assert_eq!(find(id).map(|found| found.id), Some(id));
    }
    // Every shortcut in Settings is one command's, whose keys it has.
    for hotkey in Command::ALL {
        let commands: Vec<&str> = registry::all()
            .filter(|spec| spec.hotkey == Some(hotkey))
            .map(|spec| spec.id)
            .collect();
        assert_eq!(commands.len(), 1, "{hotkey:?} is {commands:?}");
        let spec = for_hotkey(hotkey).unwrap();
        assert_eq!(spec.default_keys(), hotkey.default_keys());
        assert_eq!((spec.available)(), hotkey.available(), "{hotkey:?}");
    }
    assert!(find("playback.dance").is_none());
}

#[test]
fn describes_each_command() {
    let chapter = find("playback.go_to_chapter").unwrap().describe();
    assert_eq!(
        chapter,
        json!({
            "id": "playback.go_to_chapter",
            "title": "Go to Chapter",
            "description": "Play from the start of a chapter.",
            "keywords": ["chapters", "jump", "skip"],
            "namespace": "playback",
            "scope": "tile",
            "keys": [],
            "setting": null,
            "args": [{
                "name": "number",
                "type": "integer",
                "min": 1,
                "required": true,
                "description": "Which chapter, from 1.",
            }],
            "state": true,
            "available": true,
        })
    );
    let play = find("playback.play_pause").unwrap().describe();
    assert_eq!(play["keys"], json!(["space", "p"]));
    assert_eq!(
        play["setting"], "play_pause",
        "its name in the settings file"
    );
    assert_eq!(play["state"], false);
    let all = find("playback.play_all").unwrap().describe();
    assert_eq!(all["scope"], "tiles");
    let quit = find("app.quit").unwrap().describe();
    assert_eq!(quit["keys"], json!(["secondary-q"]));
    assert_eq!(quit["scope"], "app");
    for spec in registry::all() {
        assert_eq!(spec.describe()["id"], spec.id);
    }
}

#[test]
fn an_invocation_reads_and_writes_as_json() {
    let invocation = Invocation::new("playback.go_to_chapter")
        .with_args(json!({"number": 2}))
        .in_window("2")
        .on_tile("a1")
        .from(Source::Cli);
    let json = serde_json::to_value(&invocation).unwrap();
    assert_eq!(
        json,
        json!({
            "command": "playback.go_to_chapter",
            "args": {"number": 2},
            "target": {"window": "2", "tile": "a1"},
            "source": "cli",
        })
    );
    assert_eq!(
        serde_json::from_value::<Invocation>(json).unwrap(),
        invocation
    );
    // Left out, there are none, and it's aimed at the window in front.
    let bare: Invocation =
        serde_json::from_value(json!({"command": "app.quit", "source": "palette"})).unwrap();
    assert_eq!(bare, Invocation::new("app.quit").from(Source::Palette));
    assert_eq!(
        serde_json::to_value(&bare).unwrap(),
        json!({"command": "app.quit", "source": "palette"})
    );
}

/// The players in `window`, in grid order.
fn players(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) -> Vec<Entity<PlayerView>> {
    TestAppContext::update(cx, |cx| window.read(cx).unwrap().players())
}

/// Open `files` in `window`, and wait for its tiles and what's in them to
/// be known.
fn open(
    window: WindowHandle<Workspace>,
    files: &[&str],
    cx: &mut VisualTestContext,
) -> Vec<Entity<PlayerView>> {
    let paths = files.iter().map(|file| fixture(file)).collect();
    window
        .update(cx, |workspace, _, cx| workspace.open_paths(paths, cx))
        .unwrap();
    let opened = testing::wait_until(cx, |cx| {
        TestAppContext::update(cx, |cx| {
            let players = window.read(cx).unwrap().players();
            players.len() == files.len()
                && players
                    .iter()
                    .all(|player| player.read(cx).player().is_known())
        })
    });
    assert!(opened, "the files didn't open");
    players(window, cx)
}

fn id(player: &Entity<PlayerView>, cx: &mut VisualTestContext) -> String {
    TestAppContext::update(cx, |cx| player.read(cx).id().to_string())
}

/// A window number past the last that can't be taken for the end of the
/// one window's id, `id`, either: ids are hex, and a name is tried as the
/// end of one once it's no position.
fn past_the_last(id: &str) -> String {
    (2..=9)
        .map(|n| n.to_string())
        .find(|n| !id.ends_with(n.as_str()))
        .expect("an id ends with one digit")
}

fn looping(player: &Entity<PlayerView>, cx: &mut VisualTestContext) -> bool {
    TestAppContext::update(cx, |cx| player.read(cx).player().is_looping())
}

fn muted(player: &Entity<PlayerView>, cx: &mut VisualTestContext) -> bool {
    TestAppContext::update(cx, |cx| player.read(cx).player().is_muted())
}

fn run_now(invocation: Invocation, cx: &mut VisualTestContext) -> Result<Value, Error> {
    let done = TestAppContext::update(cx, |cx| run(invocation, cx));
    cx.run_until_parked();
    done
}

#[gpui::test]
fn a_command_aimed_at_a_tile_acts_on_it_not_on_the_one_in_front(cx: &mut TestAppContext) {
    let (front, mut cx) = testing::start(cx);
    let [first, second] = <[_; 2]>::try_from(open(front, &["clip.mp4", "clip-ip.mkv"], &mut cx))
        .ok()
        .unwrap();
    let second_id = id(&second, &mut cx);
    let toggle_loop = || Invocation::new("playback.toggle_loop");

    // By id.
    assert_eq!(
        run_now(toggle_loop().on_tile(&second_id), &mut cx),
        Ok(Value::Null)
    );
    assert!(looping(&second, &mut cx));
    assert!(!looping(&first, &mut cx), "not the one in front");
    // By position, in grid order.
    run_now(toggle_loop().on_tile("2"), &mut cx).unwrap();
    assert!(!looping(&second, &mut cx));
    // By the end of its id.
    run_now(toggle_loop().on_tile(&second_id[2..]), &mut cx).unwrap();
    assert!(looping(&second, &mut cx));
    assert!(!looping(&first, &mut cx));
    // Named by neither, the one keys go to: the first.
    run_now(toggle_loop(), &mut cx).unwrap();
    assert!(looping(&first, &mut cx));
    let loops = |target: Target, cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| state("playback.toggle_loop", &target, cx))
    };
    let tile = |name: &str| Target {
        window: None,
        tile: Some(name.into()),
    };
    assert_eq!(loops(tile(&second_id), &mut cx), Ok(json!(true)));
    assert_eq!(loops(tile("1"), &mut cx), Ok(json!(true)));
    run_now(toggle_loop().on_tile("1"), &mut cx).unwrap();
    assert_eq!(loops(tile("1"), &mut cx), Ok(json!(false)));

    // Another window, behind the first, as the test platform leaves it.
    let back = cx
        .update(|_, cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    let [other] = <[_; 1]>::try_from(open(back, &["tone.m4a"], &mut cx))
        .ok()
        .unwrap();
    let other_id = id(&other, &mut cx);
    let in_front = |cx: &mut VisualTestContext| TestAppContext::update(cx, |cx| front_window(cx));
    assert_eq!(in_front(&mut cx), Some(front));

    // The window by its position, ⌘2's, and the tile keys go to in it.
    let mute = || Invocation::new("audio.toggle_mute");
    run_now(mute().in_window("2"), &mut cx).unwrap();
    assert!(muted(&other, &mut cx));
    assert!(!muted(&first, &mut cx), "not the window in front's");
    // A tile in any window, by id.
    run_now(mute().on_tile(&other_id), &mut cx).unwrap();
    assert!(!muted(&other, &mut cx));
    // A position counts in the window in front, or the one named.
    run_now(mute().on_tile("1"), &mut cx).unwrap();
    assert!(muted(&first, &mut cx));
    assert!(!muted(&other, &mut cx));
    let back_id = TestAppContext::update(&cx, |cx| back.read(cx).unwrap().id().to_string());
    run_now(mute().in_window(&back_id).on_tile("1"), &mut cx).unwrap();
    assert!(muted(&other, &mut cx));
    assert_eq!(
        TestAppContext::update(&cx, |cx| state("audio.toggle_mute", &tile(&other_id), cx)),
        Ok(json!(true))
    );
    assert_eq!(in_front(&mut cx), Some(front), "none brought to the front");

    // A window brought to the front by its position, whose tiles positions
    // count in then.
    run_now(Invocation::new("window.focus").in_window("2"), &mut cx).unwrap();
    assert_eq!(in_front(&mut cx), Some(back));
    run_now(mute().on_tile("1"), &mut cx).unwrap();
    assert!(!muted(&other, &mut cx));
    assert!(muted(&first, &mut cx), "not the first window's");
    // As commands named nothing are.
    run_now(mute(), &mut cx).unwrap();
    assert!(muted(&other, &mut cx));
    // Every tile in a window.
    run_now(Invocation::new("playback.play_all").in_window("1"), &mut cx).unwrap();
    let playing = |player: &Entity<PlayerView>, cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| player.read(cx).is_playing())
    };
    assert!(playing(&first, &mut cx) && playing(&second, &mut cx));
    // A tile closed in a window behind.
    run_now(Invocation::new("tile.close").on_tile(&second_id), &mut cx).unwrap();
    assert_eq!(players(front, &mut cx), [first]);
}

#[gpui::test]
fn goes_to_a_chapter_by_its_number(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &["clip-chapters.mkv"], &mut cx);
    let go = |number: usize| {
        Invocation::new("playback.go_to_chapter").with_args(json!({"number": number}))
    };
    let at = |cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| {
            state("playback.go_to_chapter", &Target::default(), cx)
        })
    };
    assert_eq!(at(&mut cx), Ok(json!(1)), "counted from 1");
    assert_eq!(run_now(go(3), &mut cx), Ok(Value::Null), "the last");
    assert_eq!(at(&mut cx), Ok(json!(3)));
    assert_eq!(
        run_now(go(4), &mut cx),
        Err(Error::not_found("chapter", "4"))
    );
    assert_eq!(run_now(go(1), &mut cx), Ok(Value::Null));
    assert_eq!(at(&mut cx), Ok(json!(1)));
}

#[gpui::test]
fn closes_the_window_in_front_settings_too_or_the_one_named(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let back = cx
        .update(|_, cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    TestAppContext::update(&cx, SettingsView::open_window);
    let settings = cx
        .windows()
        .into_iter()
        .find(|window| window.downcast::<SettingsView>().is_some())
        .unwrap();
    let mut in_settings = VisualTestContext::from_window(settings, &cx);
    in_settings.update(|window, _| window.activate_window());
    in_settings.run_until_parked();
    let open = |cx: &mut VisualTestContext| cx.windows().len();
    assert_eq!(open(&mut cx), 3);

    run_now(Invocation::new("window.close"), &mut cx).unwrap();
    assert!(
        !cx.windows().contains(&settings),
        "Settings, in front, closed"
    );
    assert_eq!(open(&mut cx), 2, "the players stay");
    run_now(Invocation::new("window.close").in_window("2"), &mut cx).unwrap();
    let ids = |cx: &mut VisualTestContext| {
        cx.windows()
            .iter()
            .map(AnyWindowHandle::window_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&mut cx), [window.window_id()], "the second player");
    assert!(!ids(&mut cx).contains(&back.window_id()));
}

#[gpui::test]
fn each_kind_of_error_comes_back(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let bad = |message: &str| Err(Error::bad_args("playback.go_to_chapter", message));
    let chapter = |args: Value| Invocation::new("playback.go_to_chapter").with_args(args);
    let window_id = TestAppContext::update(&cx, |cx| window.read(cx).unwrap().id().to_string());

    assert_eq!(
        run_now(Invocation::new("playback.dance"), &mut cx),
        Err(Error::NoSuchCommand {
            command: "playback.dance".into()
        })
    );
    // Arguments, before anything else.
    assert_eq!(run_now(chapter(Value::Null), &mut cx), bad("needs number"));
    assert_eq!(
        run_now(chapter(json!({"number": 0})), &mut cx),
        bad("number is at least 1")
    );
    assert_eq!(
        run_now(chapter(json!({"number": 1, "at": 2})), &mut cx),
        bad("takes no at: it takes number")
    );
    assert_eq!(
        run_now(Invocation::new("app.quit").in_window("1"), &mut cx),
        Err(Error::bad_args(
            "app.quit",
            "acts on the app, not on a window or tile"
        ))
    );
    assert_eq!(
        run_now(Invocation::new("tile.focus.left").on_tile("1"), &mut cx),
        Err(Error::bad_args(
            "tile.focus.left",
            "acts on a window: name it with window, not tile"
        ))
    );
    assert_eq!(
        run_now(
            Invocation::new("output.set_device").with_args(json!({"name": "Radio"})),
            &mut cx
        ),
        Err(Error::bad_args("output.set_device", "name goes with an id"))
    );

    // What it's aimed at.
    let play = || Invocation::new("playback.play_pause");
    assert_eq!(
        run_now(play(), &mut cx),
        Err(Error::not_found("tile", format!("in window {window_id}"))),
        "an empty window"
    );
    let past = past_the_last(&window_id);
    assert_eq!(
        run_now(play().in_window(&past), &mut cx),
        Err(Error::not_found("window", past))
    );
    open(window, &["clip.mp4"], &mut cx);
    assert_eq!(run_now(play(), &mut cx), Ok(Value::Null));
    assert_eq!(
        run_now(play().on_tile("zz"), &mut cx),
        Err(Error::not_found("tile", "zz"))
    );
    assert_eq!(
        run_now(chapter(json!({"number": 1})), &mut cx),
        Err(Error::not_found("chapter", "1")),
        "a file without chapters"
    );
    assert_eq!(
        run_now(
            Invocation::new("output.set_device").with_args(json!({"id": "nowhere"})),
            &mut cx
        ),
        Err(Error::not_found("output", "nowhere"))
    );
    assert_eq!(
        run_now(
            Invocation::new("cast.to").with_args(json!({"device": "tv"})),
            &mut cx
        ),
        Err(Error::not_found("device", "tv"))
    );
    // Picture in Picture is macOS's.
    let pip = run_now(Invocation::new("tile.toggle_picture_in_picture"), &mut cx);
    if !cfg!(target_os = "macos") {
        assert_eq!(
            pip,
            Err(Error::failed("Picture in Picture isn't on this system"))
        );
    }
}

#[gpui::test]
fn says_what_can_run_and_what_its_state_is(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let can = |command: &str, target: Target, cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| can_run(command, &target, cx))
    };
    let none = Target::default;
    let named = |window: &str| Target {
        window: Some(window.into()),
        tile: None,
    };
    assert!(can("app.quit", none(), &mut cx));
    assert!(!can("app.quit", named("1"), &mut cx), "it takes none");
    assert!(!can("playback.dance", none(), &mut cx));
    assert!(can("window.toggle_full_screen", none(), &mut cx));
    assert!(can("playback.play_all", named("1"), &mut cx));
    let window_id = TestAppContext::update(&cx, |cx| window.read(cx).unwrap().id().to_string());
    let past = past_the_last(&window_id);
    assert!(!can("playback.play_all", named(&past), &mut cx), "no other");
    assert!(!can("playback.play_pause", none(), &mut cx), "no file");
    assert!(can("window.close", none(), &mut cx));
    open(window, &["clip.mp4"], &mut cx);
    assert!(can("playback.play_pause", none(), &mut cx));
    assert_eq!(
        can("tile.toggle_picture_in_picture", none(), &mut cx),
        cfg!(target_os = "macos")
    );

    let now = |command: &str, cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| state(command, &Target::default(), cx))
    };
    assert_eq!(
        now("playback.play_pause", &mut cx),
        Ok(Value::Null),
        "none to read"
    );
    assert_eq!(now("window.toggle_float_on_top", &mut cx), Ok(json!(false)));
    run_now(Invocation::new("window.toggle_float_on_top"), &mut cx).unwrap();
    assert_eq!(now("window.toggle_float_on_top", &mut cx), Ok(json!(true)));
    assert_eq!(
        now("window.toggle_float_on_top_while_playing", &mut cx),
        Ok(json!(false))
    );
    run_now(Invocation::new("window.toggle_float_on_top"), &mut cx).unwrap();

    assert_eq!(now("video.set_crop", &mut cx), Ok(Value::Null));
    run_now(
        Invocation::new("video.set_crop").with_args(json!({"crop": "16:9"})),
        &mut cx,
    )
    .unwrap();
    assert_eq!(now("video.set_crop", &mut cx), Ok(json!("16:9")));
    assert_eq!(
        run_now(
            Invocation::new("video.set_crop").with_args(json!({"crop": "5:4"})),
            &mut cx
        ),
        Err(Error::bad_args(
            "video.set_crop",
            "crop is one of 1:1, 4:3, 16:9, 1.85:1, 2.39:1"
        ))
    );
    run_now(
        Invocation::new("video.set_aspect_ratio").with_args(json!({"ratio": "4:3"})),
        &mut cx,
    )
    .unwrap();
    assert_eq!(now("video.set_aspect_ratio", &mut cx), Ok(json!("4:3")));
    run_now(Invocation::new("video.set_aspect_ratio"), &mut cx).unwrap();
    assert_eq!(now("video.set_aspect_ratio", &mut cx), Ok(Value::Null));

    assert_eq!(now("output.set_device", &mut cx), Ok(Value::Null));
    assert_eq!(now("output.set_window_device", &mut cx), Ok(Value::Null));
    assert_eq!(now("cast.to", &mut cx), Ok(Value::Null));
    assert_eq!(now("playback.go_to_chapter", &mut cx), Ok(Value::Null));
    assert_eq!(now("window.toggle_full_screen", &mut cx), Ok(json!(false)));
    assert_eq!(
        TestAppContext::update(&cx, |cx| state("app.quit", &named("1"), cx)),
        Err(Error::bad_args(
            "app.quit",
            "acts on the app, not on a window or tile"
        ))
    );
}

#[test]
fn scopes_are_what_their_commands_act_on() {
    let scope = |id: &str| find(id).unwrap().scope;
    assert_eq!(scope("app.quit"), Scope::App);
    assert_eq!(scope("window.close"), Scope::AnyWindow);
    assert_eq!(scope("window.focus"), Scope::Window);
    assert_eq!(scope("tile.close_all"), Scope::Tiles);
    assert_eq!(scope("playback.play_pause"), Scope::Tile);
    assert_eq!(scope("tile.close"), Scope::Tile);
}
