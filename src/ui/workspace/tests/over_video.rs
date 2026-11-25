//! The panels over the video, each shown or hidden in a block, in each
//! block in a window, or in every window as Settings → Appearance has them:
//! hidden bars aren't drawn, and hidden notes and the track switcher leave
//! unsaid what a shortcut changed, but not what's wrong.

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
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::theme::motion::MOUSE_IDLE;
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

/// `window`'s first block.
fn block(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Entity<PlayerView> {
    window
        .read_with(cx, |workspace, _| workspace.players()[0].clone())
        .unwrap()
}

/// What `block` shows over its video.
fn in_block(block: &Entity<PlayerView>, cx: &VisualTestContext) -> OverVideo {
    block.read_with(cx, |view, _| view.over_video())
}

/// What `window` shows over its blocks' videos, as those opened in it start
/// out.
fn in_window(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> OverVideo {
    window
        .read_with(cx, |workspace, _| workspace.over_video())
        .unwrap()
}

/// Run `command` on `block`, by its name.
fn on_block(block: &Entity<PlayerView>, command: &str, cx: &mut VisualTestContext) {
    let name = block.read_with(cx, |view, _| view.id().to_string());
    let done = TestAppContext::update(cx, |cx| {
        api::run(Invocation::new(command).on_block(name), cx)
    });
    done.unwrap();
    cx.run_until_parked();
}

/// Run `command` on `window`, by its name.
fn on_window(window: WindowHandle<Workspace>, command: &str, cx: &mut VisualTestContext) {
    let name = window
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap();
    let done = TestAppContext::update(cx, |cx| {
        api::run(Invocation::new(command).in_window(name), cx)
    });
    done.unwrap();
    cx.run_until_parked();
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

#[gpui::test]
fn settings_show_or_hide_each_panel_in_every_window_and_block(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let block = block(window, &cx);
    let other = open_player(&mut cx, &["clip.mp4"]);
    let elsewhere = other
        .read_with(&cx, |workspace, _| workspace.players()[0].clone())
        .unwrap();
    let all = OverVideo::default();
    assert!(all.controls && all.top_bar && all.track_switcher && all.notes);
    assert_eq!(shown(&mut cx), all, "each shows at first");
    let panels: [(&str, Panel); 4] = [
        ("app.toggle_controls", |s| &mut s.controls),
        ("app.toggle_top_bar", |s| &mut s.top_bar),
        ("app.toggle_track_switcher", |s| &mut s.track_switcher),
        ("app.toggle_notes", |s| &mut s.notes),
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
        for (seen, what) in [
            (in_window(window, &cx), "this window"),
            (in_window(other, &cx), "the other"),
            (in_block(&block, &cx), "this block"),
            (in_block(&elsewhere, &cx), "the other's"),
        ] {
            assert_eq!(seen, hidden, "{command}: {what} too");
        }
        run(&mut cx, command).unwrap();
        assert_eq!(shown(&mut cx), all, "{command} shows it again");
        assert_eq!(in_block(&elsewhere, &cx), all, "{command}: everywhere");
    }
}

#[gpui::test]
fn a_block_shows_or_hides_its_own_panels(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-subs.mkv"], false);
    let blocks = window
        .read_with(&cx, |workspace, _| workspace.players())
        .unwrap();
    let (block, beside) = (&blocks[0], &blocks[1]);
    let all = OverVideo::default();

    on_block(block, "block.toggle_controls", &mut cx);
    on_block(block, "block.toggle_notes", &mut cx);
    let own = OverVideo {
        controls: false,
        notes: false,
        ..all
    };
    assert_eq!(in_block(block, &cx), own);
    assert_eq!(in_block(beside, &cx), all, "the one beside keeps its own");
    assert_eq!(in_window(window, &cx), all, "and its window");
    assert_eq!(shown(&mut cx), all, "and Settings");
    let state = |block: &Entity<PlayerView>, cx: &mut VisualTestContext| {
        let name = block.read_with(cx, |view, _| view.id().to_string());
        TestAppContext::update(cx, |cx| {
            let target = Target {
                block: Some(name),
                ..Target::default()
            };
            api::state("block.toggle_controls", &target, cx)
        })
    };
    assert_eq!(state(block, &mut cx), Ok(json!(false)));
    assert_eq!(state(beside, &mut cx), Ok(json!(true)));

    on_block(block, "block.toggle_ui", &mut cx);
    assert_eq!(
        in_block(block, &cx),
        OverVideo::NONE,
        "any shown: all hidden"
    );
    on_block(block, "block.toggle_ui", &mut cx);
    assert_eq!(in_block(block, &cx), all, "none shown: all shown");
    assert_eq!(in_block(beside, &cx), all);
}

#[gpui::test]
fn each_command_shows_or_hides_its_own_panel_in_a_block_and_a_window(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let block = block(window, &cx);
    let all = OverVideo::default();
    let panels: [(&str, Panel); 4] = [
        ("controls", |s| &mut s.controls),
        ("top_bar", |s| &mut s.top_bar),
        ("track_switcher", |s| &mut s.track_switcher),
        ("notes", |s| &mut s.notes),
    ];
    for (name, panel) in panels {
        let mut hidden = all;
        *panel(&mut hidden) = false;
        let command = format!("block.toggle_{name}");
        on_block(&block, &command, &mut cx);
        assert_eq!(in_block(&block, &cx), hidden, "{command}");
        assert_eq!(in_window(window, &cx), all, "{command}");
        on_block(&block, &command, &mut cx);
        assert_eq!(in_block(&block, &cx), all, "{command} again");

        let command = format!("window.toggle_{name}");
        on_window(window, &command, &mut cx);
        assert_eq!(in_window(window, &cx), hidden, "{command}");
        assert_eq!(in_block(&block, &cx), hidden, "{command}");
        on_window(window, &command, &mut cx);
        assert_eq!(in_block(&block, &cx), all, "{command} again");
    }
}

#[gpui::test]
fn a_window_shows_or_hides_its_blocks_panels_and_those_opened_in_it(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-subs.mkv"], false);
    let blocks = window
        .read_with(&cx, |workspace, _| workspace.players())
        .unwrap();
    let other = open_player(&mut cx, &["clip.mp4"]);
    let elsewhere = other
        .read_with(&cx, |workspace, _| workspace.players()[0].clone())
        .unwrap();
    let all = OverVideo::default();
    // One block shows the top bar but not the notes, on its own.
    on_block(&blocks[1], "block.toggle_top_bar", &mut cx);
    on_block(&blocks[1], "block.toggle_notes", &mut cx);

    on_window(window, "window.toggle_top_bar", &mut cx);
    let hidden = OverVideo {
        top_bar: false,
        ..all
    };
    assert_eq!(in_window(window, &cx), hidden);
    assert_eq!(in_block(&blocks[0], &cx), hidden, "each block in it");
    assert_eq!(
        in_block(&blocks[1], &cx),
        OverVideo {
            notes: false,
            ..hidden
        },
        "keeping what else it has its own"
    );
    assert_eq!(in_window(other, &cx), all, "not the other window");
    assert_eq!(in_block(&elsewhere, &cx), all, "nor its blocks");
    assert_eq!(shown(&mut cx), all, "nor Settings");

    // One opened in it starts out as it is.
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![testing::fixture("clip-chapters.mkv")], cx);
        })
        .unwrap();
    let three = testing::wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, _| workspace.players().len() == 3)
            .unwrap()
    });
    assert!(three, "it opens");
    let opened = window
        .read_with(&cx, |workspace, _| workspace.players()[2].clone())
        .unwrap();
    assert_eq!(in_block(&opened, &cx), hidden);

    on_window(window, "window.toggle_ui", &mut cx);
    assert_eq!(
        in_window(window, &cx),
        OverVideo::NONE,
        "any shown: all hidden"
    );
    for block in [&blocks[0], &blocks[1], &opened] {
        assert_eq!(in_block(block, &cx), OverVideo::NONE);
    }
    on_window(window, "window.toggle_ui", &mut cx);
    assert_eq!(in_block(&blocks[1], &cx), all, "none shown: all shown");
    assert_eq!(in_window(other, &cx), all);
}

#[gpui::test]
fn every_window_and_block_follows_what_settings_change_and_keeps_the_rest(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "clip-subs.mkv"], false);
    let blocks = window
        .read_with(&cx, |workspace, _| workspace.players())
        .unwrap();
    let all = OverVideo::default();
    // The window hides the controls, but one block shows them again; the
    // other hides its notes.
    on_window(window, "window.toggle_controls", &mut cx);
    on_block(&blocks[0], "block.toggle_controls", &mut cx);
    on_block(&blocks[1], "block.toggle_notes", &mut cx);

    run(&mut cx, "app.toggle_controls").unwrap();
    let no_controls = OverVideo {
        controls: false,
        ..all
    };
    assert_eq!(in_window(window, &cx), no_controls);
    assert_eq!(in_block(&blocks[0], &cx), no_controls, "hidden everywhere");
    assert_eq!(
        in_block(&blocks[1], &cx),
        OverVideo {
            notes: false,
            ..no_controls
        },
        "the notes as they were"
    );

    run(&mut cx, "app.toggle_ui").unwrap();
    assert_eq!(shown(&mut cx), OverVideo::NONE, "any shown: all hidden");
    assert_eq!(in_window(window, &cx), OverVideo::NONE);
    assert_eq!(in_block(&blocks[1], &cx), OverVideo::NONE);
    run(&mut cx, "app.toggle_ui").unwrap();
    assert_eq!(shown(&mut cx), all, "none shown: all shown");
    assert_eq!(in_block(&blocks[1], &cx), all);

    // A window opened later starts out as Settings have it.
    run(&mut cx, "app.toggle_track_switcher").unwrap();
    let other = open_player(&mut cx, &["clip.mp4"]);
    let switcher_hidden = OverVideo {
        track_switcher: false,
        ..all
    };
    assert_eq!(in_window(other, &cx), switcher_hidden);
    let opened = other
        .read_with(&cx, |workspace, _| workspace.players()[0].clone())
        .unwrap();
    assert_eq!(in_block(&opened, &cx), switcher_hidden);

    // What a window has of its own stays as Settings change another.
    on_window(other, "window.toggle_notes", &mut cx);
    run(&mut cx, "app.toggle_top_bar").unwrap();
    let kept = OverVideo {
        top_bar: false,
        notes: false,
        ..switcher_hidden
    };
    assert_eq!(in_window(other, &cx), kept);
    assert_eq!(in_block(&opened, &cx), kept);
}

#[gpui::test]
fn hidden_bars_are_not_drawn_and_the_mouse_is_not_over_them(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-subs.mkv"], false);
    let block = block(window, &cx);
    let over_bars = |cx: &mut VisualTestContext| block.read_with(cx, |view, _| view.over_bars());
    let clearance =
        |cx: &mut VisualTestContext| block.read_with(cx, |view, _| view.subtitle_clearance());
    assert!(
        clearance(&mut cx) > px(0.),
        "subtitles keep clear of the controls"
    );
    hover(&mut cx, "block-0");
    for bar in ["block-title", "maximize", "seek", "volume"] {
        assert!(shows(&mut cx, bar), "{bar} shows at first");
    }

    hover(&mut cx, "seek");
    assert!(over_bars(&mut cx));
    run(&mut cx, "block.toggle_controls").unwrap();
    assert!(!shows(&mut cx, "seek") && !shows(&mut cx, "volume"));
    assert!(!over_bars(&mut cx), "nor over what isn't there");
    assert_eq!(clearance(&mut cx), px(0.), "nor of what isn't there");
    assert!(shows(&mut cx, "block-title") && shows(&mut cx, "maximize"));

    hover(&mut cx, "maximize");
    assert!(over_bars(&mut cx));
    run(&mut cx, "block.toggle_top_bar").unwrap();
    assert!(!shows(&mut cx, "block-title") && !shows(&mut cx, "maximize"));
    assert!(!over_bars(&mut cx));

    run(&mut cx, "block.toggle_controls").unwrap();
    hover(&mut cx, "block-0");
    assert!(shows(&mut cx, "seek") && !shows(&mut cx, "maximize"));
    // The tracks menu goes with the controls, and is closed when they're
    // back.
    click(&mut cx, "tracks");
    assert!(shows(&mut cx, "track-0"), "the menu opens");
    run(&mut cx, "block.toggle_controls").unwrap();
    assert!(!shows(&mut cx, "track-0"));
    run(&mut cx, "block.toggle_controls").unwrap();
    hover(&mut cx, "block-0");
    assert!(shows(&mut cx, "tracks") && !shows(&mut cx, "track-0"));
    run(&mut cx, "block.toggle_top_bar").unwrap();
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
    let blocks = window
        .read_with(&cx, |workspace, _| workspace.players())
        .unwrap();
    let noted = |block: &Entity<PlayerView>, cx: &mut VisualTestContext| {
        block.read_with(cx, |view, _| view.noted())
    };
    let reels = |block: &Entity<PlayerView>, cx: &mut VisualTestContext| {
        block.read_with(cx, |view, _| view.reels())
    };
    let subtitles = |view: &PlayerView| {
        view.player()
            .info()
            .and_then(|info| info.selected(TrackKind::Subtitle))
    };
    let on = |block: &Entity<PlayerView>, command: &str, cx: &mut VisualTestContext| {
        let name = block.read_with(cx, |view, _| view.id().to_string());
        TestAppContext::update(cx, |cx| {
            api::run(Invocation::new(command).on_block(name), cx)
        })
        .unwrap();
        cx.run_until_parked();
    };
    let (block, chapters) = (&blocks[0], &blocks[1]);
    run(&mut cx, "app.toggle_notes").unwrap();
    run(&mut cx, "app.toggle_track_switcher").unwrap();

    on(block, "audio.toggle_mute", &mut cx);
    assert!(
        block.read_with(&cx, |view, _| view.player().is_muted()),
        "it changed"
    );
    on(block, "audio.toggle_mute", &mut cx);
    for command in CHANGES {
        on(block, command, &mut cx);
        assert_eq!(noted(block, &mut cx), None, "{command}: unsaid");
    }
    on(chapters, "playback.next_chapter", &mut cx);
    assert_eq!(noted(chapters, &mut cx), None, "the chapter: unsaid");
    let before = block.read_with(&cx, |view, _| subtitles(view));
    on(block, "subtitles.next_track", &mut cx);
    let switched = testing::wait_until(&mut cx, |cx| {
        block.read_with(cx, |view, _| subtitles(view)) != before
    });
    assert!(switched, "the track changed");
    assert!(
        !reels(block, &mut cx) && noted(block, &mut cx).is_none(),
        "unshown"
    );
    on(block, "playback.next_chapter", &mut cx);
    assert_eq!(
        noted(block, &mut cx).as_deref(),
        Some("No chapters"),
        "why nothing happened"
    );

    run(&mut cx, "app.toggle_notes").unwrap();
    for command in CHANGES {
        let before = noted(block, &mut cx);
        on(block, command, &mut cx);
        let note = noted(block, &mut cx);
        assert!(note.is_some() && note != before, "{command}: {note:?}");
    }
    on(chapters, "playback.next_chapter", &mut cx);
    assert!(noted(chapters, &mut cx).is_some(), "the chapter");
    run(&mut cx, "app.toggle_track_switcher").unwrap();
    on(block, "subtitles.next_track", &mut cx);
    assert!(reels(block, &mut cx));
}

#[gpui::test]
fn a_screenshot_saved_is_unsaid_with_notes_hidden_but_one_failed_is_not(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let block = block(window, &cx);
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
        let name = block.read_with(cx, |view, _| view.id().to_string());
        TestAppContext::update(cx, |cx| {
            api::run(Invocation::new("video.take_screenshot").on_block(name), cx)
        })
        .unwrap();
    };
    let noted = |cx: &mut VisualTestContext| block.read_with(cx, |view, _| view.noted());
    // Besides the file that isn't a folder.
    let saved = || fs::read_dir(&folder).unwrap().count() - 1;
    // A frame to take, once one is drawn.
    let start = Instant::now();
    while block.read_with(&cx, |view, _| view.player().frame().is_none()) {
        assert!(start.elapsed() < OPEN_WAIT, "no picture to take");
        shows(&mut cx, "block-0");
        std::thread::sleep(Duration::from_millis(5));
    }

    shoot(&folder, false, &mut cx);
    assert!(testing::wait_until(&mut cx, |_| saved() == 1), "saved");
    assert_eq!(noted(&mut cx), None, "saved, unsaid");

    shoot(&not_a_folder, false, &mut cx);
    let failed = testing::wait_until(&mut cx, |cx| {
        block.read_with(cx, |view, _| view.noted()).is_some()
    });
    assert!(failed, "it says it failed");
    let note = noted(&mut cx).unwrap();
    assert!(note.starts_with("Screenshot failed"), "{note}");

    shoot(&folder, true, &mut cx);
    let said = testing::wait_until(&mut cx, |cx| {
        block
            .read_with(cx, |view, _| view.noted())
            .is_some_and(|note| note.contains("saved"))
    });
    assert!(said, "saved, said so: {:?}", noted(&mut cx));
    let _ = fs::remove_dir_all(&folder);
}

/// The mouse resting over a playing picture, as gpui's clock tells, hides
/// the pointer and fades the controls; moved, they're back.
#[gpui::test]
fn a_mouse_resting_over_the_picture_fades_the_controls(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], true);
    let middle = gpui::point(WINDOW_SIZE.width / 2., WINDOW_SIZE.height / 2.);
    cx.simulate_mouse_move(middle, None, gpui::Modifiers::none());
    cx.run_until_parked();
    let resting = |cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, cx| {
                workspace.blocks[0].view.read(cx).layout().mouse_idle
            })
            .unwrap()
    };
    assert!(!resting(&cx));
    cx.executor().advance_clock(MOUSE_IDLE / 2);
    cx.run_until_parked();
    assert!(!resting(&cx), "not for long enough");
    cx.executor().advance_clock(MOUSE_IDLE);
    cx.run_until_parked();
    assert!(resting(&cx));
    cx.simulate_mouse_move(
        middle + gpui::point(px(5.), px(5.)),
        None,
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(!resting(&cx), "moved");

    // Moved again halfway, it rests a whole while from then.
    cx.executor().advance_clock(MOUSE_IDLE / 2);
    cx.simulate_mouse_move(middle, None, gpui::Modifiers::none());
    cx.executor().advance_clock(MOUSE_IDLE * 3 / 4);
    cx.run_until_parked();
    assert!(!resting(&cx), "from the last move");
    cx.executor().advance_clock(MOUSE_IDLE / 2);
    cx.run_until_parked();
    assert!(resting(&cx));
}

#[gpui::test]
fn set_ui_shows_just_the_panels_named_and_inspect_says_which(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], false);
    let blocks = window
        .read_with(&cx, |workspace, _| workspace.players())
        .unwrap();
    let name = window
        .read_with(&cx, |workspace, _| workspace.id().to_string())
        .unwrap();
    let set = |cx: &mut VisualTestContext, invocation: Invocation| {
        let done = TestAppContext::update(cx, |cx| api::run(invocation, cx));
        cx.run_until_parked();
        done.unwrap()
    };
    let notes = OverVideo {
        notes: true,
        ..OverVideo::NONE
    };

    // The first block's controls hidden of its own: the window's set shows
    // the same in each, whatever each showed.
    on_block(&blocks[0], "block.toggle_controls", &mut cx);
    let window_set = |visible: Value| {
        Invocation::new("window.set_ui")
            .with_args(json!({ "visible": visible }))
            .in_window(name.clone())
    };
    set(&mut cx, window_set(json!(["notes"])));
    assert_eq!(in_window(window, &cx), notes);
    for block in &blocks {
        assert_eq!(in_block(block, &cx), notes);
    }
    set(&mut cx, window_set(json!(OverVideo::PANELS)));
    for block in &blocks {
        assert_eq!(
            in_block(block, &cx),
            OverVideo::default(),
            "each, the first too"
        );
    }
    // None, and again none: as asked, not flipped.
    for _ in 0..2 {
        set(&mut cx, window_set(json!([])));
        assert_eq!(in_window(window, &cx), OverVideo::NONE);
        assert_eq!(in_block(&blocks[1], &cx), OverVideo::NONE);
    }

    // A block's, the other's left as it is.
    let block_name = blocks[0].read_with(&cx, |view, _| view.id().to_string());
    let block_set = Invocation::new("block.set_ui")
        .with_args(json!({"visible": ["top_bar", "controls"]}))
        .on_block(block_name.clone());
    set(&mut cx, block_set);
    let bars = OverVideo {
        controls: true,
        top_bar: true,
        ..OverVideo::NONE
    };
    assert_eq!(in_block(&blocks[0], &cx), bars);
    assert_eq!(in_block(&blocks[1], &cx), OverVideo::NONE);

    // As inspect says, and the commands' state.
    let inspected = set(
        &mut cx,
        Invocation::new("block.inspect").on_block(block_name.clone()),
    );
    assert_eq!(inspected["visible"], json!(["controls", "top_bar"]));
    let inspected = set(
        &mut cx,
        Invocation::new("window.inspect").in_window(name.clone()),
    );
    assert_eq!(inspected["visible"], json!([]));
    let state = |command: &str, target: Target, cx: &mut VisualTestContext| {
        TestAppContext::update(cx, |cx| api::state(command, &target, cx)).unwrap()
    };
    let target = Target {
        window: None,
        block: Some(block_name),
    };
    assert_eq!(
        state("block.set_ui", target, &mut cx),
        json!(["controls", "top_bar"])
    );
    let target = Target {
        window: Some(name),
        block: None,
    };
    assert_eq!(state("window.set_ui", target, &mut cx), json!([]));
}
