use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn any_text_is_a_time_from_the_start_or_none(typed in any::<String>()) {
        if let Some(seconds) = seconds(&typed) {
            prop_assert!(seconds.is_finite() && seconds >= 0.0, "{seconds}");
        }
    }

    #[test]
    fn a_time_typed_in_hours_minutes_or_seconds_is_read(
        hours in 0u32..1000,
        minutes in 0u32..60,
        whole in 0u32..60,
        hundredths in 0u32..100,
    ) {
        let second = f64::from(whole) + f64::from(hundredths) / 100.0;
        let at = f64::from(hours) * 3600.0 + f64::from(minutes) * 60.0 + second;
        let typed = format!("{hours}:{minutes:02}:{whole:02}.{hundredths:02}");
        prop_assert!((seconds(&typed).unwrap() - at).abs() < 1e-6, "{typed}");
        let typed = format!("{}:{whole}.{hundredths:02}", hours * 60 + minutes);
        prop_assert!((seconds(&typed).unwrap() - at).abs() < 1e-6, "{typed}");
        let typed = at.to_string();
        prop_assert!((seconds(&typed).unwrap() - at).abs() < 1e-6, "{typed}");
    }

    #[test]
    fn minutes_or_seconds_past_59_are_no_time(hours in 0u32..100, over in 60u32..1000) {
        prop_assert_eq!(seconds(&format!("{hours}:{over}:00")), None);
        prop_assert_eq!(seconds(&format!("{hours}:00:{over}")), None);
    }
}
