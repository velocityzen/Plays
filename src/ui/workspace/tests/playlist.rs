//! Several files opened at once, as Settings → Playback says: a block
//! each, all playing, or one block playing them one after another, its
//! previous and next buttons going through them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::{Value, json};

use super::open;
use crate::api::{self, Invocation};
use crate::files;
use crate::media::source::media_in;
use crate::recents;
use crate::settings::{Settings, SeveralFiles};
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, OPEN_WAIT, click, fixture, hover, painted, wait_until};
use crate::ui::workspace::Workspace;

/// The app's clock, a step on: the window ticks its blocks, which go on to
/// the next file once one has played to its end.
const STEP: Duration = Duration::from_millis(100);

/// Let what plays play on until `done`, the app's clock moving with the
/// time, a `STEP` at a time; whether it was done in time.
fn play_until(
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) -> bool {
    for _ in 0..100 {
        cx.executor().advance_clock(STEP);
        cx.run_until_parked();
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Let what plays play on for `steps` of the app's clock, a second all
/// told for 10.
fn play_on(cx: &mut VisualTestContext, steps: usize) {
    for _ in 0..steps {
        cx.executor().advance_clock(STEP);
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Have several files opened at once play one after another in a block.
fn in_turn(cx: &mut VisualTestContext) {
    cx.update(|_, cx| Settings::update(cx, false, |s| s.several_files = SeveralFiles::Playlist));
}

/// Open `paths` in `window` at once, and wait until the one block they
/// open in plays its first.
fn open_in_turn(
    window: WindowHandle<Workspace>,
    cx: &mut VisualTestContext,
    paths: Vec<PathBuf>,
) -> Entity<PlayerView> {
    window
        .update(cx, |workspace, _, cx| workspace.open_paths(paths, cx))
        .unwrap();
    let opened = wait_until(cx, |cx| {
        window
            .read_with(cx, |workspace, _| !workspace.blocks.is_empty())
            .unwrap()
    });
    assert!(opened, "nothing opened in {OPEN_WAIT:?}");
    let blocks = window
        .read_with(cx, |workspace, _| workspace.players())
        .unwrap();
    assert_eq!(blocks.len(), 1, "all in one block");
    let block = blocks[0].clone();
    plays(&block, cx);
    block
}

/// Wait until `block` has opened what it plays now, and plays it.
fn plays(block: &Entity<PlayerView>, cx: &mut VisualTestContext) {
    let playing = wait_until(cx, |cx| {
        block.read_with(cx, |view, _| {
            let player = view.player();
            player.is_known() && player.is_playing()
        })
    });
    assert!(playing, "it didn't open and play in {OPEN_WAIT:?}");
}

/// The name of the file `block` plays, and where that is in its
/// playlist, as its header says.
fn at(block: &Entity<PlayerView>, cx: &VisualTestContext) -> (String, Option<String>) {
    block.read_with(cx, |view, _| {
        let name = view
            .file()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        (name, view.playlist_counter())
    })
}

fn playing(name: &str, counter: Option<&str>) -> (String, Option<String>) {
    (name.to_owned(), counter.map(str::to_owned))
}

/// Play the file before (`-1`) or after (`1`) in `block` instead, as
/// Previous File and Next File do; what it notes, if that's nowhere.
fn step(block: &Entity<PlayerView>, cx: &mut VisualTestContext, direction: i32) -> Option<String> {
    block.update(cx, |view, cx| view.open_neighbor(direction, cx));
    cx.run_until_parked();
    block.read_with(cx, |view, _| view.noted())
}

/// Wait until `block` has looked through its file's folder, so a
/// previous or next file there would be known.
fn knows_its_folder(block: &Entity<PlayerView>, cx: &mut VisualTestContext) {
    let known = wait_until(cx, |cx| {
        block.read_with(cx, |view, _| view.knows_neighbors())
    });
    assert!(known, "its folder wasn't looked through");
}

/// Seek `block` to just before its file's end.
fn near_the_end(block: &Entity<PlayerView>, cx: &mut VisualTestContext) {
    block.update(cx, |view, cx| {
        let duration = view.duration().expect("its length");
        view.seek_to(duration - 0.2, cx);
    });
    cx.run_until_parked();
}

/// `block` played its file to the end, and stopped there.
fn ended(block: &Entity<PlayerView>, cx: &VisualTestContext) -> bool {
    block.read_with(cx, |view, _| {
        !view.is_playing()
            && view
                .duration()
                .is_some_and(|duration| view.position() >= duration - 0.05)
    })
}

#[gpui::test]
fn several_files_open_in_a_block_each_by_default(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], true);
    let blocks = window
        .read_with(&cx, |workspace, _| workspace.players())
        .unwrap();
    assert_eq!(
        blocks
            .iter()
            .map(|block| at(block, &cx))
            .collect::<Vec<_>>(),
        [playing("clip.mp4", None), playing("tone.m4a", None)]
    );
}

#[gpui::test]
fn several_files_play_in_turn_in_a_block_as_settings_say(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    in_turn(&mut cx);
    let files = ["clip.mp4", "tone.m4a", "clip-subs.mkv"];
    let block = open_in_turn(window, &mut cx, files.map(fixture).to_vec());
    assert_eq!(at(&block, &cx), playing("clip.mp4", Some("1 of 3")));
    hover(&mut cx, "block-0");
    assert!(
        painted(&mut cx).iter().any(|(text, _)| text == "1 of 3"),
        "its header says where it is"
    );

    // Its folder has a file before it; its playlist doesn't.
    knows_its_folder(&block, &mut cx);
    assert_eq!(
        step(&block, &mut cx, -1).as_deref(),
        Some("No previous file in its playlist")
    );
    assert_eq!(at(&block, &cx), playing("clip.mp4", Some("1 of 3")));
    step(&block, &mut cx, 1);
    plays(&block, &mut cx);
    assert_eq!(at(&block, &cx), playing("tone.m4a", Some("2 of 3")));
    step(&block, &mut cx, -1);
    plays(&block, &mut cx);
    assert_eq!(at(&block, &cx), playing("clip.mp4", Some("1 of 3")));

    // Played to its end, the next one plays: it was playing, whatever
    // Settings say of files that open.
    cx.update(|_, cx| Settings::update(cx, false, |s| s.autoplay = false));
    near_the_end(&block, &mut cx);
    let next = play_until(&mut cx, |cx| {
        at(&block, cx).0 == "tone.m4a" && block.read_with(cx, |view, _| view.is_playing())
    });
    assert!(next, "the next file didn't play: {:?}", at(&block, &cx));
    assert_eq!(at(&block, &cx), playing("tone.m4a", Some("2 of 3")));

    step(&block, &mut cx, 1);
    plays(&block, &mut cx);
    assert_eq!(at(&block, &cx), playing("clip-subs.mkv", Some("3 of 3")));
    assert_eq!(
        step(&block, &mut cx, 1).as_deref(),
        Some("No next file in its playlist")
    );

    // The last one played to its end, the playlist is done.
    near_the_end(&block, &mut cx);
    assert!(play_until(&mut cx, |cx| ended(&block, cx)), "it didn't end");
    play_on(&mut cx, 10);
    assert_eq!(at(&block, &cx), playing("clip-subs.mkv", Some("3 of 3")));
    assert!(ended(&block, &cx), "it stays at its end");
}

#[gpui::test]
fn a_file_moved_to_its_end_while_paused_stays(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    in_turn(&mut cx);
    let block = open_in_turn(
        window,
        &mut cx,
        vec![fixture("clip.mp4"), fixture("tone.m4a")],
    );
    block.update(&mut cx, |view, cx| {
        view.pause();
        let duration = view.duration().expect("its length");
        view.seek_to(duration, cx);
    });
    play_on(&mut cx, 10);
    assert!(ended(&block, &cx), "it's at its end");
    assert_eq!(at(&block, &cx), playing("clip.mp4", Some("1 of 2")));
}

#[gpui::test]
fn a_folder_opened_plays_its_files_in_turn_as_settings_say(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    in_turn(&mut cx);
    let folder = fixture("");
    let files = media_in(&folder);
    let block = open_in_turn(window, &mut cx, vec![folder]);
    let first = files[0].file_name().unwrap().to_string_lossy();
    let counter = format!("1 of {}", files.len());
    assert_eq!(at(&block, &cx), playing(&first, Some(&counter)));
}

#[gpui::test]
fn a_playlist_file_plays_what_it_lists_in_turn_in_a_block_of_its_own(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let dir = files::test_folder("playlist-file-opened");
    std::fs::create_dir_all(&dir).unwrap();
    let list = dir.join("list.m3u");
    let lines = ["clip-subs.mkv", "tone.m4a"].map(|name| fixture(name).display().to_string());
    std::fs::write(&list, format!("#EXTM3U\n{}\n", lines.join("\n"))).unwrap();

    // Whatever Settings say of several files.
    let block = open_in_turn(window, &mut cx, vec![list.clone()]);
    assert_eq!(at(&block, &cx), playing("clip-subs.mkv", Some("1 of 2")));
    step(&block, &mut cx, 1);
    plays(&block, &mut cx);
    assert_eq!(at(&block, &cx), playing("tone.m4a", Some("2 of 2")));
    let noted = cx.update(|_, cx| recents::FILES.list(cx));
    assert!(
        noted.contains(&list.display().to_string()),
        "the playlist is noted as opened: {noted:?}"
    );
}

#[gpui::test]
fn a_file_played_instead_moves_its_playlist_or_ends_it(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    in_turn(&mut cx);
    let files = ["clip.mp4", "tone.m4a", "clip-subs.mkv"];
    let block = open_in_turn(window, &mut cx, files.map(fixture).to_vec());
    block.update(&mut cx, |view, cx| {
        view.play_instead(fixture("clip-subs.mkv"), cx);
    });
    plays(&block, &mut cx);
    assert_eq!(at(&block, &cx), playing("clip-subs.mkv", Some("3 of 3")));

    // One not in it: what plays isn't in a playlist now, and the files
    // before and after are its folder's.
    block.update(&mut cx, |view, cx| {
        view.play_instead(fixture("clip-opus.mkv"), cx);
    });
    plays(&block, &mut cx);
    assert_eq!(at(&block, &cx), playing("clip-opus.mkv", None));
    knows_its_folder(&block, &mut cx);
    step(&block, &mut cx, -1);
    plays(&block, &mut cx);
    assert_eq!(at(&block, &cx), playing("clip-one-key.mp4", None));
}

#[gpui::test]
fn previous_and_next_go_as_far_as_its_playlist(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    in_turn(&mut cx);
    let block = open_in_turn(
        window,
        &mut cx,
        vec![fixture("clip.mp4"), fixture("tone.m4a")],
    );
    // Its folder has files before and after it.
    knows_its_folder(&block, &mut cx);
    let name = block.read_with(&cx, |view, _| view.id().to_string());
    let sent = |cx: &mut VisualTestContext, control: &'static str| {
        hover(cx, "block-0");
        click(cx, control);
        cx.run_until_parked();
        cx.update(|_, cx| api::recorded(cx))
    };
    cx.update(|_, cx| api::intercept(cx));
    assert_eq!(sent(&mut cx, "previous"), [], "none before the first");
    assert_eq!(
        sent(&mut cx, "next"),
        [Invocation::new("block.playback.next_file")
            .with_args(Value::Null)
            .on_block(name.clone())]
    );
    // As Next File would.
    block.update(&mut cx, |view, cx| {
        view.play_instead(fixture("tone.m4a"), cx)
    });
    plays(&block, &mut cx);
    assert_eq!(sent(&mut cx, "next"), [], "none after the last");
    let previous = || {
        Invocation::new("block.playback.previous_file")
            .with_args(Value::Null)
            .on_block(name.clone())
    };
    assert_eq!(sent(&mut cx, "previous"), [previous()]);

    // Out of it, they go by its folder, where the first file has none
    // before it.
    block.update(&mut cx, |view, cx| {
        view.play_instead(fixture("clip-chapters.mkv"), cx)
    });
    plays(&block, &mut cx);
    knows_its_folder(&block, &mut cx);
    assert_eq!(
        sent(&mut cx, "previous"),
        [],
        "none before the folder's first"
    );
    assert_eq!(
        sent(&mut cx, "next"),
        [Invocation::new("block.playback.next_file")
            .with_args(Value::Null)
            .on_block(name.clone())]
    );
}

#[gpui::test]
fn open_takes_a_folder_s_videos_or_sound_files_together_or_in_turn_whatever_settings_say(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    let folder = fixture("");
    let open = |kind: &str, several: &str, cx: &mut VisualTestContext| {
        let invocation = Invocation::new("app.open").with_args(json!({
            "urls": [folder],
            "kind": kind,
            "several_files": several,
        }));
        TestAppContext::update(cx, |cx| api::run(invocation, cx)).unwrap();
    };
    let opened = |count: usize, cx: &mut VisualTestContext| {
        let all = wait_until(cx, |cx| {
            window
                .read_with(cx, |workspace, _| workspace.players().len() == count)
                .unwrap()
        });
        assert!(all, "{count} blocks didn't open in {OPEN_WAIT:?}");
        window
            .read_with(cx, |workspace, _| workspace.players())
            .unwrap()
    };

    in_turn(&mut cx);
    open("audio", "together", &mut cx);
    let blocks = opened(3, &mut cx);
    let names: Vec<String> = blocks.iter().map(|block| at(block, &cx).0).collect();
    assert_eq!(names, ["tone.m4a", "tune.flac", "western-tags.wav"]);
    assert!(
        blocks.iter().all(|block| at(block, &cx).1.is_none()),
        "each in a block of its own, Settings saying in turn"
    );

    cx.update(|_, cx| Settings::update(cx, false, |s| s.several_files = SeveralFiles::Together));
    open("video", "playlist", &mut cx);
    let block = opened(4, &mut cx).pop().expect("the videos' block");
    plays(&block, &mut cx);
    let videos = media_in(&folder).len() - names.len();
    let counter = format!("1 of {videos}");
    assert_eq!(
        at(&block, &cx),
        playing("clip-chapters.mkv", Some(&counter))
    );
}
