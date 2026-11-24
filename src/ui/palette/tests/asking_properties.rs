use hegel::TestCase;
use hegel::generators as gs;

use super::*;

#[hegel::test]
fn any_text_is_a_time_from_the_start_or_none(tc: TestCase) {
    if let Some(seconds) = seconds(&tc.draw(gs::text())) {
        assert!(seconds.is_finite() && seconds >= 0.0, "{seconds}");
    }
}

#[hegel::test]
fn a_time_typed_in_hours_minutes_or_seconds_is_read(tc: TestCase) {
    let hours = tc.draw(gs::integers::<u32>().max_value(999));
    let minutes = tc.draw(gs::integers::<u32>().max_value(59));
    let whole = tc.draw(gs::integers::<u32>().max_value(59));
    let hundredths = tc.draw(gs::integers::<u32>().max_value(99));
    let second = f64::from(whole) + f64::from(hundredths) / 100.0;
    let at = f64::from(hours) * 3600.0 + f64::from(minutes) * 60.0 + second;
    let typed = format!("{hours}:{minutes:02}:{whole:02}.{hundredths:02}");
    assert!((seconds(&typed).unwrap() - at).abs() < 1e-6, "{typed}");
    let typed = format!("{}:{whole}.{hundredths:02}", hours * 60 + minutes);
    assert!((seconds(&typed).unwrap() - at).abs() < 1e-6, "{typed}");
    let typed = at.to_string();
    assert!((seconds(&typed).unwrap() - at).abs() < 1e-6, "{typed}");
}

#[hegel::test]
fn minutes_or_seconds_past_59_are_no_time(tc: TestCase) {
    let hours = tc.draw(gs::integers::<u32>().max_value(99));
    let over = tc.draw(gs::integers::<u32>().min_value(60).max_value(999));
    assert_eq!(seconds(&format!("{hours}:{over}:00")), None);
    assert_eq!(seconds(&format!("{hours}:00:{over}")), None);
}
