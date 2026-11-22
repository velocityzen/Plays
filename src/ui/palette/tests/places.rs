//! Go to… (⌘P): what it lists, finding a place, and where Return goes.

use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use serde_json::json;

use super::Kind;
use crate::api::{self, Invocation, Source, Target};
use crate::files;
use crate::recents;
use crate::ui::palette::tests::{
    aimed_at, clear, open, open_palette, palette, press_and_wait, ran, read, type_in,
};
use crate::ui::palette::{GO_TO, Palette};
use crate::ui::testing::{self, fixture, press, shows, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// Open Go to… over the window in front with its keys.
fn open_go_to(cx: &mut VisualTestContext) {
    press(cx, "secondary-p");
    cx.run_until_parked();
}

/// What it lists, by kind and name.
fn places(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<(Kind, String)> {
    read(window, cx, Palette::places_listed)
}

/// The names of the places of `kind` in `listed`.
fn of(listed: &[(Kind, String)], kind: Kind) -> Vec<&str> {
    listed
        .iter()
        .filter(|(listed, _)| *listed == kind)
        .map(|(_, name)| name.as_str())
        .collect()
}

/// The file `window`'s front tile plays.
fn front_file(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Option<String> {
    window
        .read_with(cx, |workspace, cx| {
            let player = workspace.active_player()?;
            Some(files::name(player.read(cx).file()?))
        })
        .unwrap()
}

/// Wait until `window`'s front tile has looked through its folder.
fn wait_for_folder(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) {
    let looked = wait_until(cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace
                    .active_player()
                    .is_some_and(|player| player.read(cx).knows_neighbors())
            })
            .unwrap()
    });
    assert!(looked, "its folder is looked through");
}

/// A second window, labeled `label`, behind the first, and its target.
fn second_window(label: &str, cx: &mut VisualTestContext) -> (WindowHandle<Workspace>, Target) {
    let second = TestAppContext::update(cx, |cx| Workspace::open_window(WINDOW_SIZE, cx)).unwrap();
    let id = second
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap();
    let target = Target {
        window: Some(id),
        tile: None,
    };
    let mut rename = Invocation::new("window.rename").with_args(json!({"label": label}));
    rename.target = target.clone();
    TestAppContext::update(cx, |cx| api::run(rename, cx)).unwrap();
    cx.run_until_parked();
    (second, target)
}

#[gpui::test]
fn its_keys_list_the_windows_their_tiles_the_files_in_the_folder_and_those_played(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-subs.mkv"]);
    let (stage, _) = second_window("Stage", &mut cx);
    // In front in its window, but not in this one.
    stage
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![fixture("tone.m4a")], cx)
        })
        .unwrap();
    let opened = wait_until(&mut cx, |cx| {
        stage
            .read_with(cx, |workspace, _| workspace.players().len() == 1)
            .unwrap()
    });
    assert!(opened);
    wait_for_folder(window, &mut cx);
    let front = front_file(window, &cx).unwrap();

    open_go_to(&mut cx);
    assert!(shows(&mut cx, "palette"));
    assert!(!read(window, &cx, Palette::lists_commands));
    let listed = places(window, &cx);
    assert_eq!(of(&listed, Kind::Window), ["Window 1", "Stage"]);
    assert_eq!(
        of(&listed, Kind::Tile),
        ["clip.mp4", "clip-subs.mkv", "tone.m4a"]
    );
    let folder = of(&listed, Kind::InFolder);
    for file in ["clip.mp4", "clip-opus.mkv", "tone.m4a"] {
        assert!(folder.contains(&file), "{file} in {folder:?}");
    }
    assert!(!folder.contains(&"clip.srt"), "media files only");
    assert!(folder.is_sorted(), "by name");
    assert_eq!(
        of(&listed, Kind::Recent),
        ["tone.m4a", "clip-subs.mkv", "clip.mp4"],
        "the latest first"
    );
    let order = [Kind::Window, Kind::Tile, Kind::InFolder, Kind::Recent];
    let kinds: Vec<usize> = listed
        .iter()
        .map(|(kind, _)| order.iter().position(|other| other == kind).unwrap())
        .collect();
    assert!(kinds.is_sorted(), "windows, tiles, the folder, then lately");
    assert_eq!(
        read(window, &cx, Palette::current),
        ["Window 1".to_owned(), front.clone(), front.clone()],
        "this window, its tile in front, and the file it plays"
    );

    // What a window has; where a tile is, there being two windows; and a
    // file's folder.
    let picked = |typed: &str, cx: &mut VisualTestContext| {
        type_in(cx, typed);
        let (_, name, detail) = read(window, cx, Palette::picked_place).unwrap();
        clear(cx);
        (name, detail)
    };
    let detail = |text: &str| Some(text.to_owned());
    assert_eq!(picked("window 1", &mut cx).1, detail("2 files"));
    assert_eq!(picked("window stage", &mut cx).1, detail("1 file"));
    assert_eq!(picked("tile clip.mp4", &mut cx).1, detail("Window 1"));
    assert_eq!(
        picked("tile tone", &mut cx),
        ("tone.m4a".to_owned(), detail("Stage"))
    );
    type_in(&mut cx, "recent clip.mp4");
    let (kind, _, folder) = read(window, &cx, Palette::picked_place).unwrap();
    assert_eq!(kind, Kind::Recent);
    let fixtures = files::shown(fixture("clip.mp4").parent().unwrap());
    assert_eq!(folder, Some(fixtures));

    press_and_wait(&mut cx, "escape");
    assert!(palette(window, &cx).is_none(), "Escape closes it");
}

#[gpui::test]
fn typing_finds_places_by_name_kind_or_where_they_are(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-subs.mkv"]);
    second_window("Stage", &mut cx);
    wait_for_folder(window, &mut cx);
    open_go_to(&mut cx);

    type_in(&mut cx, "subs");
    let subs = "clip-subs.mkv".to_owned();
    let mut found = places(window, &cx);
    found.sort_by_key(|(kind, _)| *kind as u8);
    assert_eq!(
        found,
        [
            (Kind::Tile, subs.clone()),
            (Kind::InFolder, subs.clone()),
            (Kind::Recent, subs.clone())
        ],
        "by name"
    );
    clear(&mut cx);
    type_in(&mut cx, "recent subs");
    assert_eq!(places(window, &cx), [(Kind::Recent, subs)], "and kind");
    clear(&mut cx);
    type_in(&mut cx, "stg");
    assert_eq!(
        places(window, &cx).first(),
        Some(&(Kind::Window, "Stage".to_owned())),
        "letters apart"
    );
    clear(&mut cx);
    type_in(&mut cx, "fixtures");
    let by_folder = places(window, &cx);
    assert!(!by_folder.is_empty());
    assert!(
        by_folder.iter().all(|(kind, _)| *kind == Kind::Recent),
        "by their folder: {by_folder:?}"
    );
    clear(&mut cx);
    type_in(&mut cx, "clip.mp4");
    let found = places(window, &cx);
    assert_eq!(
        of(&found, Kind::InFolder),
        ["clip.mp4", "clip-one-key.mp4"],
        "best first, not by name"
    );
    clear(&mut cx);
    type_in(&mut cx, "nowhere at all");
    assert!(places(window, &cx).is_empty());
    assert!(shows(&mut cx, "palette-empty"));
}

#[gpui::test]
fn return_goes_to_a_tile_or_window_plays_a_file_in_the_folder_or_opens_one_played(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-subs.mkv"]);
    let (_, stage) = second_window("Stage", &mut cx);
    wait_for_folder(window, &mut cx);
    let here = aimed_at(window, &cx);
    cx.update(|_, cx| api::record(cx));
    let palette_ran = |command: &str, args: serde_json::Value, target: Target| {
        let mut invocation = Invocation::new(command)
            .with_args(args)
            .from(Source::Palette);
        invocation.target = target;
        invocation
    };

    // The tile not in front.
    let front = front_file(window, &cx).unwrap();
    let other = if front == "clip.mp4" {
        "clip-subs.mkv"
    } else {
        "clip.mp4"
    };
    let other_id = window
        .read_with(&cx, |workspace, cx| {
            workspace
                .players()
                .into_iter()
                .find(|player| player.read(cx).file().map(files::name).as_deref() == Some(other))
                .map(|player| player.read(cx).id().to_string())
        })
        .unwrap()
        .unwrap();
    open_go_to(&mut cx);
    type_in(&mut cx, &format!("tile {other}"));
    press_and_wait(&mut cx, "enter");
    assert!(palette(window, &cx).is_none(), "it closes");
    let tile = Target {
        tile: Some(other_id),
        ..here.clone()
    };
    assert_eq!(ran(&mut cx), [palette_ran("tile.focus", json!(null), tile)]);
    assert_eq!(front_file(window, &cx).as_deref(), Some(other), "in front");

    // A file in its folder, in its place.
    wait_for_folder(window, &mut cx);
    open_go_to(&mut cx);
    type_in(&mut cx, "folder opus");
    press_and_wait(&mut cx, "enter");
    let opus = fixture("clip-opus.mkv");
    assert_eq!(
        ran(&mut cx),
        [palette_ran(
            "playback.play_file",
            json!({"path": opus}),
            here.clone()
        )]
    );
    assert_eq!(front_file(window, &cx).as_deref(), Some("clip-opus.mkv"));
    let tiles = |cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.players().len())
            .unwrap()
    };
    assert_eq!(tiles(&cx), 2, "none added");

    // One played lately, in a tile of its own.
    open_go_to(&mut cx);
    type_in(&mut cx, "recent clip.mp4");
    press_and_wait(&mut cx, "enter");
    let mp4 = fixture("clip.mp4");
    assert_eq!(
        ran(&mut cx),
        [palette_ran(
            "app.open",
            json!({"urls": [mp4]}),
            here.clone()
        )]
    );
    let opened = wait_until(&mut cx, |cx| {
        let cx = VisualTestContext::from_window(window.into(), cx);
        tiles(&cx) == 3
    });
    assert!(opened, "it opens in the window");

    // Another window.
    open_go_to(&mut cx);
    type_in(&mut cx, "window stage");
    press_and_wait(&mut cx, "enter");
    assert_eq!(
        ran(&mut cx),
        [palette_ran("window.focus", json!(null), stage)]
    );
}

#[gpui::test]
fn it_and_the_command_palette_take_each_others_place(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"]);
    open_palette(&mut cx);
    assert!(read(window, &cx, Palette::lists_commands));
    type_in(&mut cx, "go to…");
    assert_eq!(read(window, &cx, Palette::picked).as_deref(), Some(GO_TO));
    press_and_wait(&mut cx, "enter");
    assert!(
        !read(window, &cx, Palette::lists_commands),
        "the palette runs it"
    );

    type_in(&mut cx, "tile");
    assert_eq!(
        read(window, &cx, Palette::picked_place),
        Some((Kind::Tile, "clip.mp4".to_owned(), None)),
        "a window alone isn't said"
    );
    open_go_to(&mut cx);
    assert_eq!(
        places(window, &cx),
        [(Kind::Tile, "clip.mp4".to_owned())],
        "⌘P again keeps it, with what's typed"
    );
    open_palette(&mut cx);
    assert!(
        read(window, &cx, Palette::lists_commands),
        "⇧⌘P puts the command palette in its place"
    );
    open_go_to(&mut cx);
    assert!(!read(window, &cx, Palette::lists_commands), "and back");
}

#[gpui::test]
fn files_opened_or_played_in_place_are_noted_once_the_latest_first(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let noted = |cx: &mut VisualTestContext| cx.update(|_, cx| recents::FILES.list(cx));
    let path = |file: &str| fixture(file).to_string_lossy().into_owned();
    // Another tile with `file`, and wait until there are `tiles`.
    let add = |cx: &mut VisualTestContext, file: &str, tiles: usize| {
        window
            .update(cx, |workspace, _, cx| {
                workspace.open_paths(vec![fixture(file)], cx)
            })
            .unwrap();
        let added = wait_until(cx, |cx| {
            window
                .read_with(cx, |workspace, _| workspace.players().len() == tiles)
                .unwrap()
        });
        assert!(added, "{file} opens");
    };
    add(&mut cx, "clip-subs.mkv", 1);
    assert_eq!(noted(&mut cx), [path("clip-subs.mkv")]);
    add(&mut cx, "clip.mp4", 2);
    add(&mut cx, "clip-subs.mkv", 3);
    assert_eq!(
        noted(&mut cx),
        [path("clip-subs.mkv"), path("clip.mp4")],
        "once each, the latest first"
    );

    // The next file in clip.mp4's folder, in its place.
    let tile = window
        .read_with(&cx, |workspace, _| workspace.players()[1].clone())
        .unwrap();
    let looked = wait_until(&mut cx, |cx| {
        tile.read_with(cx, |view, _| view.knows_neighbors())
    });
    assert!(looked);
    let name = tile.read_with(&cx, |view, _| view.id().to_string());
    let next = Invocation::new("playback.next_file").on_tile(name);
    TestAppContext::update(&cx, |cx| api::run(next, cx)).unwrap();
    cx.run_until_parked();
    assert_eq!(
        noted(&mut cx),
        [path("tone.m4a"), path("clip-subs.mkv"), path("clip.mp4")]
    );
}
