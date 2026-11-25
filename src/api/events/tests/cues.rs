use futures::channel::mpsc::unbounded;
use gpui::TestAppContext;

use super::*;

/// The block at `time`, moved `moves` times, in a file of 25 frames a
/// second.
pub(super) fn at(time: f64, moves: u64) -> Moment {
    Moment {
        time,
        moves: [0, moves, moves],
        duration: Some(2.0),
        frame_rate: Some(25.0),
    }
}

/// What a watch of `cue` says of the block at each of `moments`, in turn.
pub(super) fn reached(cue: At, moments: &[Moment]) -> Vec<bool> {
    let mut watch = Watch::new(cue);
    moments.iter().map(|now| watch.reached(now)).collect()
}

#[test]
fn a_cue_names_its_block_and_a_time_or_a_frame() {
    assert_eq!(
        parse("block.3fa9c2d1.position@120.5s"),
        Ok(("3fa9c2d1", At::Time(120.5)))
    );
    assert_eq!(parse("block.2.position@120s"), Ok(("2", At::Time(120.0))));
    assert_eq!(parse("block.2.position@345f"), Ok(("2", At::Frame(345))));
    assert_eq!(
        parse("block.v1.2@home.position@0f"),
        Ok(("v1.2@home", At::Frame(0))),
        "a label may hold dots and @"
    );
    for name in [
        "block.2.position@",
        "block.2.position@12",
        "block.2.position@-1s",
        "block.2.position@+1s",
        "block.2.position@1e3s",
        "block.2.position@infs",
        "block.2.position@.5s",
        "block.2.position@5.s",
        "block.2.position@1.5f",
        "block.2.position@ 5s",
        "block.2.position@99999999999999999999f",
        "block..position@5s",
        "block.2.time@5s",
        "window.2.position@5s",
    ] {
        let why = parse(name).expect_err(name);
        assert!(why.starts_with(name), "{why}");
    }
}

#[test]
fn a_cue_is_known_by_its_type() {
    assert!(is_cue("block.2.position@5s"));
    assert!(is_cue("block.2.position@soon"), "Plays says what's wrong");
    assert!(!is_cue("block.closed"));
    assert!(!is_cue("block.playback.position"));
    assert!(!is_cue("window.2.position@5s"));
}

#[test]
fn playing_on_reaches_a_cue_at_the_first_tick_at_it_or_past_it() {
    let ticks = [at(0.9, 0), at(0.96, 0), at(1.0, 0), at(1.04, 0)];
    assert_eq!(reached(At::Time(1.0), &ticks), [false, false, true, false]);
    assert_eq!(reached(At::Frame(25), &ticks), [false, false, true, false]);
    let ticks = [at(1.1, 0), at(1.23, 0), at(1.3, 0)];
    assert_eq!(reached(At::Frame(30), &ticks), [false, true, false]);
}

#[test]
fn a_block_seen_first_at_a_cue_or_past_it_reaches_it_once_back_before_it() {
    let ticks = [at(1.0, 0), at(1.1, 0)];
    assert_eq!(reached(At::Time(1.0), &ticks), [false, false]);
    // Played to the end, looped and played on.
    let ticks = [at(1.5, 0), at(2.0, 0), at(0.0, 1), at(0.5, 1), at(1.02, 1)];
    assert_eq!(
        reached(At::Time(1.0), &ticks),
        [false, false, false, false, true]
    );
}

#[test]
fn landing_on_a_cue_reaches_it_and_landing_past_it_doesnt() {
    let frame = 1.0 / 25.0;
    let ticks = [at(0.2, 0), at(1.5, 1), at(1.0 + frame, 2), at(1.0, 3)];
    assert_eq!(reached(At::Time(1.0), &ticks), [false, false, false, true]);
    // A step onto frame 25, to the middle of it.
    let ticks = [at(0.98, 0), at(1.02, 1)];
    assert_eq!(reached(At::Frame(25), &ticks), [false, true]);
    // A loop lands on the start.
    let ticks = [at(1.99, 0), at(0.0, 1)];
    assert_eq!(reached(At::Frame(0), &ticks), [false, true]);
    // A seek where it was already, as a track picked does, isn't one.
    let ticks = [at(1.0, 0), at(1.0, 1)];
    assert_eq!(reached(At::Time(1.0), &ticks), [false, false]);
}

#[test]
fn a_frame_isnt_reached_in_sound_alone_and_a_time_is_landed_on_within_a_25th() {
    let sound = |time, moves| Moment {
        frame_rate: None,
        ..at(time, moves)
    };
    let ticks = [sound(0.5, 0), sound(0.0, 1), sound(0.01, 1)];
    assert_eq!(reached(At::Frame(0), &ticks), [false; 3]);
    let ticks = [sound(0.5, 0), sound(1.03, 1), sound(0.5, 2), sound(1.05, 3)];
    assert_eq!(reached(At::Time(1.0), &ticks), [false, true, false, false]);
}

#[gpui::test]
fn a_cue_is_its_listeners_until_reached_or_its_block_closes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let block = Id::next();
        let count = |cx: &App| cx.global::<Cues>().0.len();
        let (to, heard) = unbounded();
        let (other, _other) = unbounded();
        set("a", (block, At::Time(1.0)), None, &to, cx);
        set("a", (block, At::Time(1.5)), None, &to, cx);
        set("a", (block, At::Time(1.0)), None, &other, cx);
        assert_eq!(count(cx), 2, "one each, the one set again in its place");
        assert!(watched(block, cx));
        assert!(!watched(Id::next(), cx));

        // Subscribing again, a listener keeps its cues; one gone, its go.
        let (again, heard_again) = unbounded();
        hand_over(&to, &again, cx);
        drop(heard);
        set("b", (block, At::Time(1.0)), None, &other, cx);
        assert_eq!(count(cx), 3);
        drop(heard_again);
        set("c", (block, At::Time(1.0)), None, &other, cx);
        assert_eq!(count(cx), 3, "those it handed over gone with it");

        // Reached, a cue is gone.
        see(block, at(0.5, 0), cx);
        see(block, at(1.2, 0), cx);
        assert_eq!(count(cx), 0, "{:?}", cx.global::<Cues>().0.len());

        set("d", (block, At::Time(1.0)), None, &other, cx);
        forget_closed(&[block], cx);
        assert!(watched(block, cx));
        forget_closed(&[], cx);
        assert!(!watched(block, cx));
    });
}
