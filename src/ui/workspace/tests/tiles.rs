//! The tile commands, run as the API runs them: closing the others, going
//! back to a tile clicked or gone to before, naming one, copying its
//! file's path or where it is, and the keys for them.

use gpui::{Entity, EntityId, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::{Value, json};

use super::{open, players};
use crate::api::{self, Error, Invocation, Target};
use crate::settings::Settings;
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, click, fixture, hover, press, shows};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// Run `invocation` as a deferred command does, outside any window's
/// update, and what it hands back.
fn run(cx: &mut VisualTestContext, invocation: Invocation) -> Result<Value, Error> {
    let done = TestAppContext::update(cx, |cx| api::run(invocation, cx));
    cx.run_until_parked();
    done
}

/// The API's name for `tile`.
fn name(tile: &Entity<PlayerView>, cx: &VisualTestContext) -> String {
    tile.read_with(cx, |view, _| view.id().to_string())
}

/// Run `command`, given `args`, on `tile`, and what it hands back.
fn run_on(
    cx: &mut VisualTestContext,
    command: &str,
    args: Value,
    tile: &Entity<PlayerView>,
) -> Result<Value, Error> {
    let invocation = Invocation::new(command)
        .with_args(args)
        .on_tile(name(tile, cx));
    run(cx, invocation)
}

/// `command` aimed at `window`.
fn in_window(command: &str, window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Invocation {
    let name = window
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap();
    Invocation::new(command).in_window(name)
}

/// The tiles' players, in order.
fn tiles(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<Entity<PlayerView>> {
    window
        .read_with(cx, |workspace, _| workspace.players())
        .unwrap()
}

fn ids(tiles: &[Entity<PlayerView>]) -> Vec<EntityId> {
    tiles.iter().map(Entity::entity_id).collect()
}

/// The tile keys go to.
fn in_front(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Option<EntityId> {
    window
        .read_with(cx, |workspace, _| {
            workspace.active_player().map(|tile| tile.entity_id())
        })
        .unwrap()
}

fn label(tile: &Entity<PlayerView>, cx: &VisualTestContext) -> Option<String> {
    tile.read_with(cx, |view, _| view.label().map(str::to_owned))
}

/// Open another player window, with `files` in it.
fn open_player(cx: &mut VisualTestContext, files: &[&str]) -> WindowHandle<Workspace> {
    let window = TestAppContext::update(cx, |cx| Workspace::open_window(WINDOW_SIZE, cx))
        .expect("opening a window");
    cx.run_until_parked();
    let mut other = VisualTestContext::from_window(window.into(), cx);
    open(window, &mut other, files, false);
    window
}

fn clipboard(cx: &VisualTestContext) -> Option<String> {
    TestAppContext::read(cx, |cx| cx.read_from_clipboard()?.text())
}

#[gpui::test]
fn close_others_closes_every_tile_in_the_window_but_the_one_named(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(
        window,
        &mut cx,
        &["clip.mp4", "clip-ip.mkv", "tone.m4a"],
        false,
    );
    let other = open_player(&mut cx, &["clip.mp4"]);
    let [_, b, _] = &tiles(window, &cx)[..] else {
        panic!("three tiles");
    };

    run_on(&mut cx, "tile.close_others", Value::Null, b).unwrap();
    assert_eq!(ids(&tiles(window, &cx)), [b.entity_id()]);
    assert_eq!(tiles(other, &cx).len(), 1, "another window's stay");
}

#[gpui::test]
fn back_and_recent_go_to_the_tiles_clicked_or_gone_to_before(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    // Two rows of two:
    // a b
    // c d
    let files = ["clip.mp4", "clip-ip.mkv", "clip.mp4", "tone.m4a"];
    open(window, &mut cx, &files, false);
    let [a, b, c, d] = &ids(&tiles(window, &cx))[..] else {
        panic!("four tiles");
    };
    let (a, b, c, d) = (*a, *b, *c, *d);
    let command = |command: &str, cx: &mut VisualTestContext| {
        let invocation = in_window(command, window, cx);
        run(cx, invocation).unwrap();
        in_front(window, cx)
    };

    // The mouse passing over a tile goes to it, but only those clicked
    // are gone back to.
    click(&mut cx, "tile-0");
    click(&mut cx, "tile-1");
    hover(&mut cx, "tile-2");
    hover(&mut cx, "tile-0");
    assert_eq!(in_front(window, &cx), Some(a));
    assert_eq!(command("tile.recent", &mut cx), Some(b), "not c");
    assert_eq!(command("tile.recent", &mut cx), Some(a), "and back");

    // Gone to from the keys, a tile is gone back to too.
    press(&mut cx, "secondary-alt-down");
    assert_eq!(in_front(window, &cx), Some(c));
    click(&mut cx, "tile-3");
    assert_eq!(command("tile.back", &mut cx), Some(c), "back one");
    assert_eq!(command("tile.back", &mut cx), Some(a), "and on, past it");
    assert_eq!(command("tile.back", &mut cx), Some(b));
    assert_eq!(command("tile.back", &mut cx), Some(b), "none before it");

    // Another clicked, the walk starts again, even back where it went.
    click(&mut cx, "tile-0");
    click(&mut cx, "tile-1");
    assert_eq!(
        command("tile.back", &mut cx),
        Some(a),
        "clicked just before"
    );
    // Another command, it starts again too.
    run(&mut cx, Invocation::new("window.toggle_float_on_top")).unwrap();
    assert_eq!(command("tile.back", &mut cx), Some(b), "not on to c");

    // Those closed are passed over, nothing put in front since.
    let players = tiles(window, &cx);
    run_on(&mut cx, "tile.close", Value::Null, &players[0]).unwrap();
    assert_eq!(command("tile.back", &mut cx), Some(c), "a closed");
    run_on(&mut cx, "tile.close", Value::Null, &players[1]).unwrap();
    assert_eq!(command("tile.recent", &mut cx), Some(d), "b closed");
}

#[gpui::test]
fn a_tile_moved_in_from_another_window_is_gone_back_to(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], false);
    let other = open_player(&mut cx, &["clip-ip.mkv"]);
    let moved = tiles(other, &cx)[0].clone();
    let here = window
        .read_with(&cx, |workspace, _| workspace.id().to_string())
        .unwrap();

    click(&mut cx, "tile-0");
    run_on(
        &mut cx,
        "tile.move_to_window",
        json!({"window": here}),
        &moved,
    )
    .unwrap();
    assert_eq!(in_front(window, &cx), Some(moved.entity_id()));
    click(&mut cx, "tile-1");
    let invocation = in_window("tile.recent", window, &cx);
    run(&mut cx, invocation).unwrap();
    assert_eq!(
        in_front(window, &cx),
        Some(moved.entity_id()),
        "in front, once it came"
    );
}

#[gpui::test]
fn rename_labels_a_tile_its_header_shows_and_names_it_by(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], false);
    let other = open_player(&mut cx, &["clip-ip.mkv"]);
    let [a, b] = &tiles(window, &cx)[..] else {
        panic!("two tiles");
    };
    let elsewhere = &tiles(other, &cx)[0];
    let title = |tile: &Entity<PlayerView>, cx: &VisualTestContext| {
        tile.read_with(cx, |view, _| view.header_title().to_string())
    };
    assert_eq!(title(a, &cx), "clip.mp4", "the file's name");

    run_on(&mut cx, "tile.rename", json!({"label": " Intro  "}), a).unwrap();
    assert_eq!(label(a, &cx).as_deref(), Some("Intro"), "trimmed");
    assert_eq!(title(a, &cx), "Intro", "its header shows it");
    run(
        &mut cx,
        Invocation::new("audio.toggle_mute").on_tile("Intro"),
    )
    .unwrap();
    let muted: Vec<bool> = players(window, &cx).iter().map(|p| p.2).collect();
    assert_eq!(muted, [true, false], "named by it");

    run_on(&mut cx, "tile.rename", json!({"label": "Intro"}), a).expect("its own again");
    let taken = Err(Error::bad_args(
        "tile.rename",
        "another tile is called Intro",
    ));
    for tile in [b, elsewhere] {
        let renamed = run_on(&mut cx, "tile.rename", json!({"label": "Intro"}), tile);
        assert_eq!(renamed, taken, "in whichever window");
        assert_eq!(label(tile, &cx), None);
    }

    run_on(&mut cx, "tile.rename", json!({"label": " "}), a).unwrap();
    assert_eq!(label(a, &cx), None, "blank is none");
    assert_eq!(title(a, &cx), "clip.mp4", "as it was");
}

#[gpui::test]
fn a_label_shows_in_the_header_though_titles_dont(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let tile = &tiles(window, &cx)[0];
    cx.update(|_, cx| Settings::update(cx, false, |s| s.show_titles = false));
    hover(&mut cx, "tile-0");
    assert!(!shows(&mut cx, "tile-title"), "titles are off");

    run_on(&mut cx, "tile.rename", json!({"label": "Intro"}), tile).unwrap();
    hover(&mut cx, "tile-0");
    assert!(shows(&mut cx, "tile-title"), "but it was given");
}

#[gpui::test]
fn rename_with_no_label_asks_for_one(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], false);
    let [a, b] = &tiles(window, &cx)[..] else {
        panic!("two tiles");
    };
    run_on(&mut cx, "tile.rename", json!({"label": "Song"}), b).unwrap();

    run_on(&mut cx, "tile.rename", Value::Null, a).unwrap();
    assert!(shows(&mut cx, "rename-tile"), "it asks");
    cx.simulate_input("Song ");
    press(&mut cx, "enter");
    assert!(
        shows(&mut cx, "rename-tile-error"),
        "another tile is called so, trimmed"
    );
    assert!(shows(&mut cx, "rename-tile"), "and it asks again");
    assert_eq!(label(a, &cx), None);

    press(&mut cx, "backspace backspace backspace backspace backspace");
    cx.simulate_input("Clip");
    press(&mut cx, "enter");
    cx.run_until_parked();
    assert!(!shows(&mut cx, "rename-tile"), "named, it goes");
    assert_eq!(label(a, &cx).as_deref(), Some("Clip"));
    assert_eq!(
        label(b, &cx).as_deref(),
        Some("Song"),
        "the other's as it was"
    );

    // Asked again, it holds the label, Return keeping it.
    run_on(&mut cx, "tile.rename", Value::Null, a).unwrap();
    press(&mut cx, "enter");
    cx.run_until_parked();
    assert!(!shows(&mut cx, "rename-tile"));
    assert_eq!(label(a, &cx).as_deref(), Some("Clip"), "held");
    // And Escape leaves it, whatever was typed.
    run_on(&mut cx, "tile.rename", Value::Null, a).unwrap();
    cx.simulate_input("Other");
    press(&mut cx, "escape");
    assert!(!shows(&mut cx, "rename-tile"), "Escape puts it away");
    assert_eq!(label(a, &cx).as_deref(), Some("Clip"));
}

#[gpui::test]
fn copies_the_files_path_and_where_it_is(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], false);
    let [clip, tone] = &tiles(window, &cx)[..] else {
        panic!("two tiles");
    };
    let path = |file: &str| fixture(file).display().to_string();

    let copied = run_on(&mut cx, "tile.copy_path", Value::Null, tone).unwrap();
    assert_eq!(copied, json!(path("tone.m4a")), "what was copied");
    assert_eq!(clipboard(&cx), Some(path("tone.m4a")));

    run_on(&mut cx, "playback.seek", json!({"time": 1.5}), clip).unwrap();
    let copied = run_on(&mut cx, "tile.copy_time", Value::Null, clip).unwrap();
    assert_eq!(copied, json!("0:01"));
    assert_eq!(clipboard(&cx).as_deref(), Some("0:01"));

    // ⌥⌘C copies the path of the tile in front.
    hover(&mut cx, "tile-0");
    press(&mut cx, "secondary-alt-c");
    assert_eq!(clipboard(&cx), Some(path("clip.mp4")));
}

#[gpui::test]
fn shift_command_return_maximizes_the_tile_in_front_and_move_says_where_it_is(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], false);
    let tone = tiles(window, &cx)[1].entity_id();
    let maximized = |cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.maximized)
            .unwrap()
    };

    hover(&mut cx, "tile-1");
    press(&mut cx, "secondary-shift-enter");
    assert_eq!(maximized(&cx), Some(tone));
    press(&mut cx, "secondary-shift-enter");
    assert_eq!(maximized(&cx), None, "and back");

    let place = TestAppContext::update(&cx, |cx| api::state("tile.move", &Target::default(), cx));
    assert_eq!(place, Ok(json!(2)), "its place, from 1");
}
