use hegel::TestCase;
use hegel::generators as gs;

use super::*;

#[hegel::test]
fn any_time_is_waited_for(tc: TestCase) {
    let seconds = tc.draw(gs::floats::<f64>());
    let waited = wait_for(seconds);
    if seconds.is_nan() || seconds <= 0.0 {
        assert_eq!(waited, Duration::ZERO);
    } else if seconds < 1e9 {
        assert!((waited.as_secs_f64() - seconds).abs() < 1e-6);
    } else {
        assert!(waited >= Duration::from_secs(999_999_999));
    }
}
