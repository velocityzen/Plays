//! Casting a block to the AirPlay crate's fake TV, as someone at the screen
//! would: pairing with the PIN it shows, then driving it. The stream
//! server is the real one, serving this computer only.

use std::sync::Arc;

use airplay::PinPairing;
use airplay::fake::{FakeReceiver, Kind};
use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use plist::{Dictionary, Value};
use serde_json::json;

use super::casting::{block, get, wait};
use super::open;
use crate::api::{self, Invocation};
use crate::cast::airplay::{AirPlayTarget, KIND};
use crate::cast::{AppPairings, Casting, Pairings};
use crate::media::tracks::TrackKind;
use crate::ui::testing::{self, click, press, shows};
use crate::ui::workspace::Workspace;

const PIN: &str = "4321";

/// A fake Apple TV 4K, which pairs with `PIN` and says what it's told to
/// play is 2 seconds long.
fn tv() -> FakeReceiver {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    tv.state().duration = 2.0;
    tv
}

/// The app with an AirPlay target that has found `tv`, and `file` open in
/// its window, playing or paused.
fn start(
    cx: &mut TestAppContext,
    tv: &FakeReceiver,
    file: &str,
    playing: bool,
) -> (WindowHandle<Workspace>, VisualTestContext) {
    let (window, mut cx) = testing::start(cx);
    let target = AirPlayTarget::with(vec![tv.device()], pairings(&mut cx));
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(target)])));
    open(window, &mut cx, &[file], playing);
    if !playing {
        block(window, &cx).update(&mut cx, |view, cx| {
            view.pause();
            cx.notify();
        });
    }
    (window, cx)
}

/// The app's pairings.
fn pairings(cx: &mut VisualTestContext) -> Arc<Pairings> {
    cx.update(|_, cx| Arc::clone(&cx.global::<AppPairings>().0))
}

/// Pair with `tv` as if that had been done before.
fn paired_before(cx: &mut VisualTestContext, tv: &FakeReceiver) {
    let device = tv.device();
    let credentials = PinPairing::start(&device)
        .and_then(|pairing| pairing.finish(PIN))
        .expect("paired with the fake");
    pairings(cx).keep(&device.id, &device.name, &credentials);
    tv.state().pin_shown = false;
}

/// What it was told to play, in order.
fn plays(tv: &FakeReceiver) -> Vec<Dictionary> {
    tv.state()
        .requests("POST", "/play")
        .into_iter()
        .map(|play| play.dictionary())
        .collect()
}

/// The last of those.
fn last_play(tv: &FakeReceiver) -> Dictionary {
    plays(tv).pop().expect("told to play")
}

fn number(play: &Dictionary, key: &str) -> f64 {
    play.get(key).and_then(Value::as_real).unwrap_or(f64::NAN)
}

fn url(play: &Dictionary) -> String {
    play.get("Content-Location")
        .and_then(Value::as_string)
        .unwrap_or_default()
        .to_owned()
}

/// The rates it was set to, in order.
fn rates(tv: &FakeReceiver) -> Vec<f64> {
    tv.state()
        .requests("POST", "/rate")
        .into_iter()
        .filter_map(|rate| rate.query("value"))
        .collect()
}

/// Whether it was asked what it's doing twice since it was last told to
/// play, so the first answer, saying what it plays, was heard: until then,
/// saying nothing is loading, not the end.
fn seen_playing(tv: &FakeReceiver) -> bool {
    let state = tv.state();
    let play = state
        .requests
        .iter()
        .rposition(|request| request.path() == "/play")
        .unwrap_or(0);
    state.requests[play..]
        .iter()
        .filter(|request| request.path() == "/playback-info")
        .count()
        >= 2
}

/// Pick the TV from the block's cast menu.
fn pick(cx: &mut VisualTestContext) {
    click(cx, "cast");
    click(cx, "cast-0");
}

/// Pick the TV, and wait for it to be told to play.
fn cast(cx: &mut VisualTestContext, tv: &FakeReceiver) {
    let before = plays(tv).len();
    pick(cx);
    wait(cx, "the TV to be told to play", |_| {
        plays(tv).len() > before
    });
}

/// Whether the pairing sheet waits for the PIN the TV shows.
pub(super) fn wants_pin(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) -> bool {
    window
        .read_with(cx, |workspace, cx| {
            workspace
                .pairing()
                .is_some_and(|sheet| sheet.read(cx).wants_pin())
        })
        .unwrap()
}

/// What the pairing sheet says.
pub(super) fn sheet_says(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) -> String {
    window
        .read_with(cx, |workspace, cx| {
            workspace
                .pairing()
                .map(|sheet| sheet.read(cx).says())
                .unwrap_or_default()
        })
        .unwrap()
}

#[gpui::test]
fn a_tv_asks_for_the_pin_it_shows_once_and_a_wrong_one_has_it_show_another(
    cx: &mut TestAppContext,
) {
    let tv = tv();
    let (window, mut cx) = start(cx, &tv, "clip.mp4", false);
    let name = block(window, &cx).read_with(&cx, |view, _| view.id().to_string());
    let device = tv.device();
    let on_block = |command: &str| {
        Invocation::new(command)
            .with_args(json!({"device": device.id, "kind": KIND}))
            .on_block(name.clone())
    };
    cx.update(|_, cx| api::record(cx));
    pick(&mut cx);
    wait(&mut cx, "a sheet to ask for the PIN", |cx| {
        wants_pin(window, cx)
    });
    assert_eq!(
        cx.update(|_, cx| api::recorded(cx)),
        [on_block("cast.to"), on_block("cast.pair")],
        "the menu casts; the TV wanting its PIN, the block asks to pair"
    );
    assert!(tv.state().pin_shown, "the TV shows it");
    assert!(shows(&mut cx, "pairing"));
    let says = sheet_says(window, &mut cx);
    assert_eq!(says, "Enter the PIN Fake TV shows.");
    assert!(plays(&tv).is_empty(), "not till it's paired");
    let view = block(window, &cx);
    assert!(view.read_with(&cx, |view, _| view.casting().is_none()));

    tv.state().pin_shown = false;
    cx.simulate_input("0000");
    press(&mut cx, "enter");
    wait(&mut cx, "a wrong PIN to have it show another", |cx| {
        wants_pin(window, cx) && sheet_says(window, cx).contains("wrong")
    });
    assert!(tv.state().pin_shown, "it shows another");
    assert!(tv.paired().is_empty());
    let says = sheet_says(window, &mut cx);
    assert!(says.contains("Enter the new PIN Fake TV shows."), "{says}");

    cx.simulate_input(PIN);
    press(&mut cx, "enter");
    wait(&mut cx, "it to play once paired", |_| {
        !plays(&tv).is_empty()
    });
    assert_eq!(
        cx.update(|_, cx| api::recorded(cx)),
        [on_block("cast.to")],
        "paired, it's cast there again"
    );
    assert!(!shows(&mut cx, "pairing"), "the sheet goes");
    assert_eq!(
        view.read_with(&cx, |view, _| view
            .casting()
            .map(|device| device.name.clone())),
        Some("Fake TV".to_owned())
    );
    assert_eq!(tv.paired().len(), 1);
    let kept = pairings(&mut cx).paired();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, "Fake TV");

    // Cast again, it isn't asked for.
    view.update(&mut cx, |view, cx| view.cast_to(None, cx));
    tv.state().pin_shown = false;
    cast(&mut cx, &tv);
    assert!(!tv.state().pin_shown);
    assert!(!shows(&mut cx, "pairing"));
    assert_eq!(tv.paired().len(), 1, "paired once");
}

#[gpui::test]
fn a_tv_that_turns_this_computer_away_says_what_lets_it_in(cx: &mut TestAppContext) {
    let tv = tv();
    tv.state().refuse_pairing = true;
    let (window, mut cx) = start(cx, &tv, "clip.mp4", false);
    pick(&mut cx);
    wait(&mut cx, "the sheet to say why it can't pair", |cx| {
        sheet_says(window, cx).starts_with("Can't pair")
    });
    let says = sheet_says(window, &mut cx);
    assert!(
        says.starts_with("Can't pair with Fake TV: it lets only some devices play to it."),
        "{says}"
    );
    assert!(says.contains("Home app"), "{says}");
    assert!(!tv.state().pin_shown);
}

#[gpui::test]
fn cancelling_the_sheet_leaves_the_block_playing_here(cx: &mut TestAppContext) {
    let tv = tv();
    let (window, mut cx) = start(cx, &tv, "clip.mp4", false);
    pick(&mut cx);
    wait(&mut cx, "a sheet to ask for the PIN", |cx| {
        wants_pin(window, cx)
    });
    press(&mut cx, "escape");
    wait(&mut cx, "the sheet to go", |cx| !shows(cx, "pairing"));
    let view = block(window, &cx);
    assert!(view.read_with(&cx, |view, _| view.casting().is_none()));
    assert!(pairings(&mut cx).paired().is_empty());
    assert!(plays(&tv).is_empty());
}

#[gpui::test]
fn a_paired_tv_plays_the_front_block_from_where_it_is_with_subtitles_in_the_playlist(
    cx: &mut TestAppContext,
) {
    let tv = tv();
    let (window, mut cx) = start(cx, &tv, "clip-subs.mkv", true);
    paired_before(&mut cx, &tv);
    let view = block(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx);
        view.seek(1.0);
    });

    cast(&mut cx, &tv);
    assert!(!tv.state().pin_shown, "paired already");
    let play = last_play(&tv);
    // It played on a little before it was cast.
    let here = view.read_with(&cx, |view, _| view.player().position());
    let start = number(&play, "Start-Position-Seconds");
    assert!(
        start >= 1.0 && (start - here).abs() < 0.01,
        "from where the block was: {start} against {here}"
    );
    assert_eq!(number(&play, "rate"), 1.0, "playing, as the block was");

    let url = url(&play);
    assert!(url.ends_with("master.m3u8"), "always HLS: {url}");
    let (status, master) = get(&url);
    assert_eq!(status, 200);
    let master = String::from_utf8(master).unwrap();
    assert!(
        master.contains("TYPE=SUBTITLES") && master.contains("NAME=\"English\""),
        "in the playlist, which it draws: {master}"
    );
    let base = url.trim_end_matches("master.m3u8");
    let (_, video) = get(&format!("{base}video.m3u8"));
    assert!(
        String::from_utf8(video).unwrap().contains("init.mp4"),
        "an Apple TV 4K gets fragmented MP4"
    );
    wait(&mut cx, "the block to say where it plays", |cx| {
        shows(cx, "play-here")
    });

    // Without them: given again from where it is, at a URL of its own, as
    // it reads the playlist once.
    tv.state()
        .playback
        .insert("position".into(), Value::Real(1.5));
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx);
    });
    wait(&mut cx, "it to be told to play again", |_| {
        plays(&tv).len() == 2
    });
    let again = last_play(&tv);
    assert_eq!(self::url(&again), format!("{url}?1"));
    assert_eq!(number(&again, "Start-Position-Seconds"), 1.5);
    assert_eq!(number(&again, "rate"), 1.0);
    let (_, master) = get(&url);
    assert!(
        !String::from_utf8(master).unwrap().contains("SUBTITLES"),
        "none now"
    );
}

#[gpui::test]
fn the_blocks_controls_reach_it_and_its_status_moves_the_bar(cx: &mut TestAppContext) {
    let tv = tv();
    let (window, mut cx) = start(cx, &tv, "clip.mp4", false);
    paired_before(&mut cx, &tv);
    let view = block(window, &cx);
    cast(&mut cx, &tv);
    assert_eq!(
        number(&last_play(&tv), "rate"),
        0.0,
        "paused, as the block was"
    );
    wait(&mut cx, "the block to say where it plays", |cx| {
        shows(cx, "play-here")
    });

    press(&mut cx, "space");
    wait(&mut cx, "it to play", |_| rates(&tv).last() == Some(&1.0));
    // Back, as forward goes past the end of so short a clip.
    press(&mut cx, "left");
    wait(&mut cx, "it to seek", |_| {
        tv.state()
            .requests("POST", "/scrub")
            .last()
            .is_some_and(|scrub| scrub.query("position") == Some(0.0))
    });
    press(&mut cx, "m");
    wait(&mut cx, "it to mute", |_| tv.state().volume_db <= -144.0);
    press(&mut cx, "m");
    press(&mut cx, "down");
    wait(&mut cx, "it to turn down", |_| {
        let db = tv.state().volume_db;
        db > -30.0 && db < 0.0
    });
    press(&mut cx, "space");
    wait(&mut cx, "it to pause", |_| rates(&tv).last() == Some(&0.0));
    assert!(
        !view.read_with(&cx, |view, _| view.player().is_playing()),
        "the shortcuts drive it, not the block's own playback"
    );

    tv.state()
        .playback
        .insert("position".into(), Value::Real(1.0));
    wait(&mut cx, "the bar to follow it", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.0).abs() < 1e-9)
    });
    assert!(!view.read_with(&cx, |view, _| view.is_playing()));

    // Played to the end, it says nothing of what it played.
    assert!(seen_playing(&tv));
    tv.state().playback = Dictionary::new();
    wait(&mut cx, "the bar to show the end", |cx| {
        view.read_with(cx, |view, _| {
            !view.is_playing() && (view.position() - 2.0).abs() < 1e-9
        })
    });
    assert!(
        view.read_with(&cx, |view, _| view.casting().is_some()),
        "the cast goes on at the end"
    );
    // Sent back from the end, it plays from there once told to.
    view.update(&mut cx, |view, _| view.seek(1.0));
    wait(&mut cx, "the bar to go there", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.0).abs() < 1e-9)
    });
    assert_eq!(plays(&tv).len(), 1, "not till it plays");
    press(&mut cx, "space");
    wait(&mut cx, "it to be told to play again", |_| {
        plays(&tv).len() == 2
    });
    let again = last_play(&tv);
    assert_eq!(
        number(&again, "Start-Position-Seconds"),
        1.0,
        "from where it was sent"
    );
    assert_eq!(number(&again, "rate"), 1.0);
    wait(&mut cx, "it to be turned down again", |_| {
        // Each starts at full volume.
        let state = tv.state();
        let volume = state
            .requests
            .iter()
            .rposition(|request| request.method == "SET_PARAMETER");
        let play = state
            .requests
            .iter()
            .rposition(|request| request.path() == "/play");
        volume > play
    });
    let db = tv.state().volume_db;
    assert!(db > -30.0 && db < 0.0, "as it was: {db} dB");

    wait(&mut cx, "it to be seen playing", |_| seen_playing(&tv));
    tv.state().playback = Dictionary::new();
    wait(&mut cx, "the bar to show the end", |cx| {
        view.read_with(cx, |view, _| (view.position() - 2.0).abs() < 1e-9)
    });
    press(&mut cx, "space");
    wait(&mut cx, "it to be told to play again", |_| {
        plays(&tv).len() == 3
    });
    assert_eq!(
        number(&last_play(&tv), "Start-Position-Seconds"),
        0.0,
        "from the start"
    );
}

#[gpui::test]
fn a_tv_that_forgot_this_computer_asks_for_its_pin_again(cx: &mut TestAppContext) {
    let tv = tv();
    let (window, mut cx) = start(cx, &tv, "clip.mp4", false);
    // Paired with another TV of the same id, as one reset since.
    let other = FakeReceiver::start(Kind::Tv { pin: PIN });
    let credentials = PinPairing::start(&other.device())
        .and_then(|pairing| pairing.finish(PIN))
        .expect("paired with the other");
    let device = tv.device();
    pairings(&mut cx).keep(&device.id, &device.name, &credentials);

    pick(&mut cx);
    wait(&mut cx, "a sheet to ask for the PIN", |cx| {
        wants_pin(window, cx)
    });
    assert!(tv.state().pin_shown);
    assert_eq!(
        pairings(&mut cx).credentials(&device.id).as_ref(),
        Some(&credentials),
        "kept till it's paired again"
    );
    cx.simulate_input(PIN);
    press(&mut cx, "enter");
    wait(&mut cx, "it to play once paired", |_| {
        !plays(&tv).is_empty()
    });
    assert_eq!(pairings(&mut cx).paired().len(), 1);
    assert_ne!(
        pairings(&mut cx).credentials(&device.id).as_ref(),
        Some(&credentials),
        "replaced once paired again"
    );
}
