use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn a_length_as_matroska_tags_it_reads_back(
        hours in 0u32..1000,
        minutes in 0u32..60,
        seconds in 0.0f64..60.0,
    ) {
        let text = format!("{hours:02}:{minutes:02}:{seconds:012.9}");
        let read = length_in_seconds(&text).unwrap();
        let length = f64::from(hours) * 3600.0 + f64::from(minutes) * 60.0 + seconds;
        prop_assert!((read - length).abs() < 1e-6, "{} read as {}", text, read);
    }

    #[test]
    fn any_length_read_is_a_time(text in "[ 0-9]{0,4}(:[-+ 0-9.eEinfINFa]{0,10}){0,4}") {
        if let Some(length) = length_in_seconds(&text) {
            prop_assert!(length.is_finite() && length >= 0.0, "{} read as {}", text, length);
        }
    }
}
