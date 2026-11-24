use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn any_time_is_waited_for(seconds in any::<f64>()) {
        let waited = wait_for(seconds);
        if seconds.is_nan() || seconds <= 0.0 {
            prop_assert_eq!(waited, Duration::ZERO);
        } else if seconds < 1e9 {
            prop_assert!((waited.as_secs_f64() - seconds).abs() < 1e-6);
        } else {
            prop_assert!(waited >= Duration::from_secs(999_999_999));
        }
    }
}
