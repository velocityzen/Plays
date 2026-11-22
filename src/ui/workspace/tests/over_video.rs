//! The panels over the video, each shown or hidden in every tile with a
//! `view` command, as Settings → Appearance has them: hidden bars aren't
//! drawn, and hidden notes and the track switcher leave unsaid what a
//! shortcut changed, but not what's wrong.

use std::fs;
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle, px};
use serde_json::{Value, json};

use super::open;
use crate::api::{self, Error, Invocation, Target};
use crate::files::{self, Folder};
use crate::media::tracks::TrackKind;
use crate::settings::{OverVideo, Settings};
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, OPEN_WAIT, click, hover, shows};
use crate::ui::workspace::Workspace;

/// Run `command` as a deferred command does, outside any window's update.
fn run(cx: &mut VisualTestContext, command: &str) -> Result<Value, Error> {
    let done = TestAppContext::update(cx, |cx| api::run(Invocation::new(command), cx));
    cx.run_until_parked();
    done
}

/// One of the panels over the video, picked from what shows.
type Panel = fn(&mut OverVideo) -> &mut bool;

/// What shows over the video now.
fn shown(cx: &mut VisualTestContext) -> OverVideo {
    cx.update(|_, cx| cx.global::<Settings>().over_video)
}

/// `window`'s first tile.
fn tile(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Entity<PlayerView> {
    window
        .read_with(cx, |workspace, _| workspace.players()[0].clone())
        .unwrap()
}

#[gpui::test]
fn each_panel_over_the_video_is_shown_or_hidden_on_its_own(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let all = OverVideo::default();
    assert!(all.controls && all.top_bar && all.track_switcher && all.notes);
    assert_eq!(shown(&mut cx), all, "each shows at first");
    let panels: [(&str, Panel); 4] = [
        ("view.toggle_controls", |s| &mut s.controls),
        ("view.toggle_top_bar", |s| &mut s.top_bar),
        ("view.toggle_track_switcher", |s| &mut s.track_switcher),
        ("view.toggle_notes", |s| &mut s.notes),
    ];
    let state = |command: &str, cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| api::state(command, &Target::default(), cx))
    };
    // Kept as it changes, in a settings file of the test's own.
    let folder = Folder::for_test("over-video");
    cx.update(|_, cx| cx.set_global(folder.clone()));
    let saved = || -> Value {
        let file = fs::read_to_string(folder.file("settings.json").unwrap()).unwrap();
        serde_json::from_str::<Value>(&file).unwrap()["over_video"].clone()
    };
    for (command, panel) in panels {
        assert_eq!(state(command, &mut cx), Ok(json!(true)), "{command}");
        run(&mut cx, command).unwrap();
        let mut hidden = all;
        *panel(&mut hidden) = false;
        assert_eq!(shown(&mut cx), hidden, "{command} hides its own");
        assert_eq!(saved(), json!(hidden), "{command}: kept so");
        assert_eq!(state(command, &mut cx), Ok(json!(false)), "{command}");
        run(&mut cx, command).unwrap();
        assert_eq!(shown(&mut cx), all, "{command} shows it again");
    }
}

#[gpui::test]
fn hidden_bars_are_not_drawn_and_the_mouse_is_not_over_them(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-subs.mkv"], false);
    let tile = tile(window, &cx);
    let over_bars = |cx: &mut VisualTestContext| tile.read_with(cx, |view, _| view.over_bars());
    let clearance =
        |cx: &mut VisualTestContext| tile.read_with(cx, |view, cx| view.subtitle_clearance(cx));
    assert!(
        clearance(&mut cx) > px(0.),
        "subtitles keep clear of the controls"
    );
    hover(&mut cx, "tile-0");
    for bar in ["tile-title", "maximize", "seek", "volume"] {
        assert!(shows(&mut cx, bar), "{bar} shows at first");
    }

    hover(&mut cx, "seek");
    assert!(over_bars(&mut cx));
    run(&mut cx, "view.toggle_controls").unwrap();
    assert!(!shows(&mut cx, "seek") && !shows(&mut cx, "volume"));
    assert!(!over_bars(&mut cx), "nor over what isn't there");
    assert_eq!(clearance(&mut cx), px(0.), "nor of what isn't there");
    assert!(shows(&mut cx, "tile-title") && shows(&mut cx, "maximize"));

    hover(&mut cx, "maximize");
    assert!(over_bars(&mut cx));
    run(&mut cx, "view.toggle_top_bar").unwrap();
    assert!(!shows(&mut cx, "tile-title") && !shows(&mut cx, "maximize"));
    assert!(!over_bars(&mut cx));

    run(&mut cx, "view.toggle_controls").unwrap();
    hover(&mut cx, "tile-0");
    assert!(shows(&mut cx, "seek") && !shows(&mut cx, "maximize"));
    // The tracks menu goes with the controls, and is closed when they're
    // back.
    click(&mut cx, "tracks");
    assert!(shows(&mut cx, "track-0"), "the menu opens");
    run(&mut cx, "view.toggle_controls").unwrap();
    assert!(!shows(&mut cx, "track-0"));
    run(&mut cx, "view.toggle_controls").unwrap();
    hover(&mut cx, "tile-0");
    assert!(shows(&mut cx, "tracks") && !shows(&mut cx, "track-0"));
    run(&mut cx, "view.toggle_top_bar").unwrap();
    assert!(shows(&mut cx, "maximize"), "both again");
}

/// What a shortcut changes on a file with sound, video and subtitles,
/// each saying so in a note of its own: the subtitles shown, then hidden.
const CHANGES: [&str; 10] = [
    "audio.volume_up",
    "audio.toggle_mute",
    "playback.toggle_loop",
    "subtitles.toggle",
    "subtitles.toggle",
    "audio.later",
    "subtitles.later",
    "subtitles.bigger",
    "video.rotate_right",
    "video.reset_picture",
];

#[gpui::test]
fn hidden_notes_and_switcher_leave_changes_unsaid_but_not_whats_wrong(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(
        window,
        &mut cx,
        &["clip-subs.mkv", "clip-chapters.mkv"],
        false,
    );
    let tiles = window
        .read_with(&cx, |workspace, _| workspace.players())
        .unwrap();
    let noted = |tile: &Entity<PlayerView>, cx: &mut VisualTestContext| {
        tile.read_with(cx, |view, _| view.noted())
    };
    let reels = |tile: &Entity<PlayerView>, cx: &mut VisualTestContext| {
        tile.read_with(cx, |view, _| view.reels())
    };
    let subtitles = |view: &PlayerView| {
        view.player()
            .info()
            .and_then(|info| info.selected(TrackKind::Subtitle))
    };
    let on = |tile: &Entity<PlayerView>, command: &str, cx: &mut VisualTestContext| {
        let name = tile.read_with(cx, |view, _| view.id().to_string());
        TestAppContext::update(cx, |cx| {
            api::run(Invocation::new(command).on_tile(name), cx)
        })
        .unwrap();
        cx.run_until_parked();
    };
    let (tile, chapters) = (&tiles[0], &tiles[1]);
    run(&mut cx, "view.toggle_notes").unwrap();
    run(&mut cx, "view.toggle_track_switcher").unwrap();

    on(tile, "audio.toggle_mute", &mut cx);
    assert!(
        tile.read_with(&cx, |view, _| view.player().is_muted()),
        "it changed"
    );
    on(tile, "audio.toggle_mute", &mut cx);
    for command in CHANGES {
        on(tile, command, &mut cx);
        assert_eq!(noted(tile, &mut cx), None, "{command}: unsaid");
    }
    on(chapters, "playback.next_chapter", &mut cx);
    assert_eq!(noted(chapters, &mut cx), None, "the chapter: unsaid");
    let before = tile.read_with(&cx, |view, _| subtitles(view));
    on(tile, "subtitles.next_track", &mut cx);
    let switched = testing::wait_until(&mut cx, |cx| {
        tile.read_with(cx, |view, _| subtitles(view)) != before
    });
    assert!(switched, "the track changed");
    assert!(
        !reels(tile, &mut cx) && noted(tile, &mut cx).is_none(),
        "unshown"
    );
    on(tile, "playback.next_chapter", &mut cx);
    assert_eq!(
        noted(tile, &mut cx).as_deref(),
        Some("No chapters"),
        "why nothing happened"
    );

    run(&mut cx, "view.toggle_notes").unwrap();
    for command in CHANGES {
        let before = noted(tile, &mut cx);
        on(tile, command, &mut cx);
        let note = noted(tile, &mut cx);
        assert!(note.is_some() && note != before, "{command}: {note:?}");
    }
    on(chapters, "playback.next_chapter", &mut cx);
    assert!(noted(chapters, &mut cx).is_some(), "the chapter");
    run(&mut cx, "view.toggle_track_switcher").unwrap();
    on(tile, "subtitles.next_track", &mut cx);
    assert!(reels(tile, &mut cx));
}

#[gpui::test]
fn a_screenshot_saved_is_unsaid_with_notes_hidden_but_one_failed_is_not(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let tile = tile(window, &cx);
    let folder = files::test_folder("over-video-shots");
    fs::create_dir_all(&folder).unwrap();
    let not_a_folder = folder.join("not-a-folder");
    fs::write(&not_a_folder, b"").unwrap();
    let shoot = |into: &std::path::Path, notes: bool, cx: &mut VisualTestContext| {
        let into = into.to_owned();
        cx.update(|_, cx| {
            Settings::update(cx, false, |s| {
                s.screenshots.folder = Some(into);
                s.over_video.notes = notes;
            });
        });
        let name = tile.read_with(cx, |view, _| view.id().to_string());
        TestAppContext::update(cx, |cx| {
            api::run(Invocation::new("video.take_screenshot").on_tile(name), cx)
        })
        .unwrap();
    };
    let noted = |cx: &mut VisualTestContext| tile.read_with(cx, |view, _| view.noted());
    // Besides the file that isn't a folder.
    let saved = || fs::read_dir(&folder).unwrap().count() - 1;
    // A frame to take, once one is drawn.
    let start = Instant::now();
    while tile.read_with(&cx, |view, _| view.player().frame().is_none()) {
        assert!(start.elapsed() < OPEN_WAIT, "no picture to take");
        shows(&mut cx, "tile-0");
        std::thread::sleep(Duration::from_millis(5));
    }

    shoot(&folder, false, &mut cx);
    assert!(testing::wait_until(&mut cx, |_| saved() == 1), "saved");
    assert_eq!(noted(&mut cx), None, "saved, unsaid");

    shoot(&not_a_folder, false, &mut cx);
    let failed = testing::wait_until(&mut cx, |cx| {
        tile.read_with(cx, |view, _| view.noted()).is_some()
    });
    assert!(failed, "it says it failed");
    let note = noted(&mut cx).unwrap();
    assert!(note.starts_with("Screenshot failed"), "{note}");

    shoot(&folder, true, &mut cx);
    let said = testing::wait_until(&mut cx, |cx| {
        tile.read_with(cx, |view, _| view.noted())
            .is_some_and(|note| note.contains("saved"))
    });
    assert!(said, "saved, said so: {:?}", noted(&mut cx));
    let _ = fs::remove_dir_all(&folder);
}
