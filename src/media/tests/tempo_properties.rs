use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// A low rate, so a few thousand frames make many windows.
const RATE: u32 = 8_000;

/// Up to `frames` of sound made of `pattern`, the channels apart.
fn sound(pattern: &[f32], frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| [pattern[i % pattern.len()], pattern[i * 3 % pattern.len()]])
        .collect()
}

#[hegel::test]
fn any_sound_at_any_speeds_is_heard_in_order_and_no_louder(tc: TestCase) {
    let pattern = tc.draw(
        gs::vecs(gs::floats::<f32>().min_value(-1.0).max_value(1.0))
            .min_size(1)
            .max_size(64),
    );
    let frames = tc.draw(gs::integers::<usize>().max_value(3000));
    let pieces = tc.draw(
        gs::vecs(gs::integers::<usize>().min_value(1).max_value(500))
            .min_size(1)
            .max_size(16),
    );
    let speeds = tc.draw(
        gs::vecs(
            gs::floats::<f64>()
                .min_value(MIN_SPEED)
                .max_value(MAX_SPEED),
        )
        .min_size(1)
        .max_size(16),
    );
    let sound = sound(&pattern, frames);
    let loudest = sound.iter().fold(0.0f32, |most, x| most.max(x.abs()));

    let mut stretch = Stretch::new(RATE);
    let mut fed = 0;
    let mut steps = 0;
    let mut last = f64::NEG_INFINITY;
    for &piece in pieces.iter().cycle() {
        let end = (fed + piece).min(frames);
        let pts = fed as f64 / f64::from(RATE);
        let taken = stretch.feed(pts, &sound[fed * SOURCE_CHANNELS..end * SOURCE_CHANNELS]);
        fed += taken;
        while stretch.step(speeds[steps % speeds.len()]) {
            steps += 1;
            let heard = stretch.read_pts().expect("what's ready has a time");
            assert!(heard >= last - 1e-9, "heard {heard} after {last}");
            last = heard;
            for x in stretch.ready() {
                assert!(x.abs() <= loudest + 1e-5, "{x} louder than {loudest}");
            }
            let ready = stretch.ready().len() / SOURCE_CHANNELS;
            stretch.take(ready);
        }
        assert!(stretch.wants() > 0, "it steps once fed what it wants");
        if fed == frames || taken == 0 && end > fed {
            break;
        }
    }
    assert_eq!(
        fed, frames,
        "it takes all the sound it's fed, as it wants it"
    );
}
