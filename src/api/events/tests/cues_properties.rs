use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::tests::{at, reached};
use super::*;

/// A cue in the first five seconds, or the first 150 frames; past the
/// start, with `past_start`.
fn cue(tc: &TestCase, past_start: bool) -> At {
    let least = u64::from(past_start);
    tc.draw(hegel::one_of!(
        gs::floats::<f64>()
            .min_value(if past_start { 0.001 } else { 0.0 })
            .max_value(5.0)
            .map(At::Time),
        gs::integers::<u64>()
            .min_value(least)
            .max_value(150)
            .map(At::Frame)
    ))
}

/// A frame rate, or none for sound alone.
fn frame_rate(tc: &TestCase) -> Option<f64> {
    tc.draw(gs::sampled_from(vec![
        None,
        Some(23.976),
        Some(25.0),
        Some(29.97),
        Some(59.94),
    ]))
}

/// A time in the first six seconds.
fn time(tc: &TestCase) -> f64 {
    tc.draw(gs::floats::<f64>().min_value(0.0).max_value(6.0))
}

/// Whether a block at `time`, in a file of `frame_rate`, is at `cue` or
/// past it, as the frames its events tell count.
fn at_or_past(cue: At, time: f64, frame_rate: Option<f64>) -> bool {
    match cue {
        At::Time(at) => time >= at,
        At::Frame(at) => playhead(time, None, frame_rate)["frame"]
            .as_u64()
            .is_some_and(|frame| frame >= at),
    }
}

#[hegel::test]
fn a_cue_is_never_reached_before_it(tc: TestCase) {
    let cue = cue(&tc, false);
    let mut watch = Watch::new(cue);
    let mut moves = 0;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(20)) {
        moves += u64::from(tc.draw(gs::booleans()));
        let now = Moment {
            frame_rate: frame_rate(&tc),
            ..at(time(&tc), moves)
        };
        if watch.reached(&now) {
            assert!(at_or_past(cue, now.time, now.frame_rate), "{now:?}");
        }
    }
}

#[hegel::test]
fn playing_on_reaches_a_cue_once_at_the_first_tick_at_it_or_past_it(tc: TestCase) {
    let cue = cue(&tc, true);
    let fps = frame_rate(&tc);
    tc.assume(fps.is_some() || matches!(cue, At::Time(_)));
    let mut times = tc.draw(gs::vecs(gs::floats::<f64>().min_value(0.0).max_value(6.0)));
    times.push(0.0);
    times.sort_by(f64::total_cmp);
    let ticks: Vec<Moment> = times
        .iter()
        .map(|&time| Moment {
            frame_rate: fps,
            ..at(time, 0)
        })
        .collect();
    let first = ticks.iter().position(|now| at_or_past(cue, now.time, fps));
    let expected: Vec<bool> = (0..ticks.len()).map(|tick| Some(tick) == first).collect();
    assert_eq!(reached(cue, &ticks), expected, "{cue:?}, {times:?}");
}

#[hegel::test]
fn a_seek_past_a_cue_doesnt_reach_it_as_the_block_plays_on(tc: TestCase) {
    let cue = cue(&tc, false);
    let fps = frame_rate(&tc);
    let from = time(&tc);
    let mut times = tc.draw(gs::vecs(gs::floats::<f64>().min_value(0.0).max_value(9.0)));
    times.sort_by(f64::total_cmp);
    let frame = fps.map_or(SOUND_LANDING, |fps| 1.0 / fps);
    // Past its frame, or a frame past its time.
    let past = |time: f64| match cue {
        At::Time(at) => time >= at + frame,
        At::Frame(at) => fps.is_some_and(|fps| frame_at(time, fps) > at),
    };
    let landed: Vec<f64> = times.into_iter().filter(|&time| past(time)).collect();
    let mut ticks = vec![Moment {
        frame_rate: fps,
        ..at(from, 0)
    }];
    ticks.extend(landed.iter().map(|&time| Moment {
        frame_rate: fps,
        ..at(time, 1)
    }));
    assert!(
        reached(cue, &ticks).iter().all(|&reached| !reached),
        "{cue:?}, from {from} to {landed:?}"
    );
}

#[hegel::test]
fn a_seek_landing_on_a_cue_reaches_it(tc: TestCase) {
    let cue = cue(&tc, false);
    let fps = frame_rate(&tc).or(Some(25.0));
    let frame = 1.0 / fps.unwrap_or(25.0);
    let start = match cue {
        At::Time(at) => at,
        At::Frame(at) => at as f64 * frame,
    };
    let landing = start + tc.draw(gs::floats::<f64>().min_value(0.0).max_value(0.9)) * frame;
    tc.assume(cue.against(landing, fps) == Some(Against::On));
    let from = time(&tc);
    tc.assume(from != landing);
    let ticks = [
        Moment {
            frame_rate: fps,
            ..at(from, 0)
        },
        Moment {
            frame_rate: fps,
            ..at(landing, 1)
        },
    ];
    assert_eq!(reached(cue, &ticks), [false, true], "{cue:?} from {from}");
}
