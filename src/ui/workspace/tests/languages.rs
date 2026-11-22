//! A file opens with its audio and subtitles in the first languages
//! Settings prefer that it has, and with the tracks picked in it before,
//! kept with where it was left.

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::{Value, json};

use super::open;
use crate::api::{self, Invocation};
use crate::media::picking::Picked;
use crate::settings::Settings;
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, fixture};
use crate::ui::workspace::Workspace;

/// clip-languages.mkv's English and German audio tracks.
const ENGLISH_AUDIO: usize = 1;
const GERMAN_AUDIO: usize = 2;
/// Its English and French subtitles.
const ENGLISH_SUBTITLES: usize = 3;
const FRENCH_SUBTITLES: usize = 4;

/// `window`'s first tile.
fn tile(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Entity<PlayerView> {
    window
        .read_with(cx, |workspace, _| workspace.players()[0].clone())
        .unwrap()
}

/// The audio and subtitle tracks `tile` plays.
fn playing(tile: &Entity<PlayerView>, cx: &VisualTestContext) -> (Option<usize>, Option<usize>) {
    tile.read_with(cx, |view, _| {
        let info = view.player().info().expect("known");
        (info.audio_track, info.subtitle_track)
    })
}

/// Open clip-languages.mkv in `window`, and say what it plays.
fn opened(
    window: WindowHandle<Workspace>,
    cx: &mut VisualTestContext,
) -> (Option<usize>, Option<usize>) {
    open(window, cx, &["clip-languages.mkv"], false);
    playing(&tile(window, cx), cx)
}

/// Close `window`'s tiles, which leaves where their files were.
fn close_all(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) {
    window
        .update(cx, |workspace, _, cx| workspace.close_all(cx))
        .unwrap();
    cx.run_until_parked();
}

/// Prefer `audio` and `subtitles`, as Settings → Audio and Subtitles do.
fn prefer(audio: &[&str], subtitles: &[&str], cx: &mut VisualTestContext) {
    let codes = |languages: &[&str]| languages.iter().map(|&l| l.to_owned()).collect();
    cx.update(|_, cx| {
        Settings::update(cx, false, |s| {
            s.languages.audio = codes(audio);
            s.languages.subtitles = codes(subtitles);
        });
    });
}

/// Run `command` with `args` on `tile`, as its menus do.
fn on(tile: &Entity<PlayerView>, command: &str, args: Value, cx: &mut VisualTestContext) {
    let name = tile.read_with(cx, |view, _| view.id().to_string());
    TestAppContext::update(cx, |cx| {
        api::run(Invocation::new(command).with_args(args).on_tile(name), cx)
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn a_file_opens_in_the_first_languages_preferred_it_has(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    assert_eq!(
        opened(window, &mut cx),
        (Some(ENGLISH_AUDIO), None),
        "as the file has it, with none preferred"
    );
    close_all(window, &mut cx);

    prefer(&["ita", "ger"], &["fre"], &mut cx);
    assert_eq!(
        opened(window, &mut cx),
        (Some(GERMAN_AUDIO), Some(FRENCH_SUBTITLES))
    );
    close_all(window, &mut cx);

    prefer(&["eng"], &["spa", "eng"], &mut cx);
    assert_eq!(
        opened(window, &mut cx),
        (Some(ENGLISH_AUDIO), Some(ENGLISH_SUBTITLES))
    );
}

#[gpui::test]
fn the_tracks_picked_in_a_file_play_again_as_it_opens_again(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    prefer(&["eng"], &[], &mut cx);
    assert_eq!(opened(window, &mut cx), (Some(ENGLISH_AUDIO), None));
    let first = tile(window, &cx);
    on(&first, "audio.select_track", json!({"number": 2}), &mut cx);
    on(
        &first,
        "subtitles.select_track",
        json!({"number": 2}),
        &mut cx,
    );
    close_all(window, &mut cx);
    assert_eq!(
        opened(window, &mut cx),
        (Some(GERMAN_AUDIO), Some(FRENCH_SUBTITLES)),
        "as picked, over the language preferred"
    );

    // Hidden, they stay hidden, whatever's preferred.
    on(&tile(window, &cx), "subtitles.toggle", json!({}), &mut cx);
    close_all(window, &mut cx);
    prefer(&["eng"], &["eng"], &mut cx);
    assert_eq!(opened(window, &mut cx), (Some(GERMAN_AUDIO), None));
    close_all(window, &mut cx);

    // Kept only while files resume where they were left.
    cx.update(|_, cx| Settings::update(cx, false, |s| s.resume_files = false));
    assert_eq!(
        opened(window, &mut cx),
        (Some(ENGLISH_AUDIO), Some(ENGLISH_SUBTITLES)),
        "as Settings prefer"
    );
}

#[gpui::test]
fn a_file_played_instead_opens_with_its_own_tracks(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    prefer(&["ger"], &["fre"], &mut cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let tile = tile(window, &cx);
    let play_instead = |file: &str, cx: &mut VisualTestContext| {
        tile.update(cx, |view, cx| view.play_instead(fixture(file), cx));
        let known = testing::wait_until(cx, |cx| {
            tile.read_with(cx, |view, _| view.player().is_known())
        });
        assert!(known, "{file} didn't open");
        playing(&tile, cx)
    };
    assert_eq!(
        play_instead("clip-languages.mkv", &mut cx),
        (Some(GERMAN_AUDIO), Some(FRENCH_SUBTITLES)),
        "as Settings prefer"
    );
    on(&tile, "audio.select_track", json!({"number": 1}), &mut cx);
    on(&tile, "subtitles.select_track", json!({}), &mut cx);

    play_instead("clip.mp4", &mut cx);
    let picked = tile.read_with(&cx, |view, _| view.progress().map(|note| note.picked));
    assert_eq!(picked, Some(Picked::default()), "none picked in this one");
    assert_eq!(
        play_instead("clip-languages.mkv", &mut cx),
        (Some(ENGLISH_AUDIO), None),
        "as picked in it before"
    );
}
