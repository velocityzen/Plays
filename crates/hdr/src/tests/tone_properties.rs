use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// A screen's headroom, as much as macOS gives.
fn headroom(tc: &TestCase) -> f32 {
    tc.draw(gs::floats::<f32>().min_value(1.0).max_value(16.0))
}

/// A video's peak, relative to SDR white, from reference white to PQ's.
fn peak(tc: &TestCase, at_least: f32) -> f32 {
    tc.draw(
        gs::floats::<f32>()
            .min_value(at_least)
            .max_value(PQ_PEAK_NITS / REFERENCE_WHITE_NITS),
    )
}

/// Brightness, from black to far past PQ's peak.
fn brightness(tc: &TestCase) -> f32 {
    tc.draw(gs::floats::<f32>().min_value(0.0).max_value(100.0))
}

/// As close as rounding leaves two values the curve takes the same way.
fn alike(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-5 * a.abs().max(b.abs()).max(1.0)
}

#[hegel::test]
fn brightness_keeps_its_order_and_fits_the_headroom(tc: TestCase) {
    let headroom = headroom(&tc);
    let peak = tc.draw(gs::booleans()).then(|| peak(&tc, 1.0));
    let mut all = tc.draw(gs::vecs(
        gs::floats::<f32>().min_value(0.0).max_value(100.0),
    ));
    all.sort_by(f32::total_cmp);
    let curved: Vec<f32> = all.iter().map(|&x| tone_curve(x, headroom, peak)).collect();
    assert!(
        curved
            .windows(2)
            .all(|pair| pair[0] <= pair[1] || alike(pair[0], pair[1])),
        "{all:?} became {curved:?}"
    );
    for (&x, &y) in all.iter().zip(&curved) {
        assert!(y <= headroom, "{x} became {y}, over {headroom}");
        assert!(y <= x || alike(x, y), "{x} brightened to {y}");
        if x <= KNEE * headroom {
            assert_eq!(x, y, "under the knee");
        }
    }
}

#[hegel::test]
fn a_known_peak_reaches_the_headroom_there_and_only_there(tc: TestCase) {
    let headroom = headroom(&tc);
    let peak = peak(&tc, headroom * 1.01);
    assert!(alike(tone_curve(peak, headroom, Some(peak)), headroom));
    let knee = KNEE * headroom;
    let under = knee + (peak - knee) * 0.9;
    assert!(tone_curve(under, headroom, Some(peak)) < headroom);
    let over = peak + brightness(&tc);
    assert!(alike(tone_curve(over, headroom, Some(peak)), headroom));
}

#[hegel::test]
fn a_peak_under_the_headroom_leaves_brightness_alone(tc: TestCase) {
    let headroom = headroom(&tc);
    let peak = tc.draw(gs::floats::<f32>().min_value(0.5).max_value(headroom));
    let x = tc.draw(gs::floats::<f32>().min_value(0.0).max_value(headroom));
    assert!(alike(tone_curve(x, headroom, Some(peak)), x));
}

#[hegel::test]
fn brighter_masters_leave_more_room(tc: TestCase) {
    let headroom = headroom(&tc);
    let dimmer = peak(&tc, 1.0);
    let brighter = peak(&tc, dimmer);
    let x = brightness(&tc);
    let (from_brighter, from_dimmer) = (
        tone_curve(x, headroom, Some(brighter)),
        tone_curve(x, headroom, Some(dimmer)),
    );
    assert!(
        from_brighter <= from_dimmer || alike(from_brighter, from_dimmer),
        "{x}: {from_brighter} from {brighter}, {from_dimmer} from {dimmer}"
    );
}
