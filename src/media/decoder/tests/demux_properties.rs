use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;

#[hegel::test]
fn a_length_as_matroska_tags_it_reads_back(tc: TestCase) {
    let hours = tc.draw(gs::integers::<u32>().max_value(999));
    let minutes = tc.draw(gs::integers::<u32>().max_value(59));
    let seconds = tc.draw(gs::floats::<f64>().min_value(0.0).max_value_exclusive(60.0));
    let text = format!("{hours:02}:{minutes:02}:{seconds:012.9}");
    let read = length_in_seconds(&text).unwrap();
    let length = f64::from(hours) * 3600.0 + f64::from(minutes) * 60.0 + seconds;
    assert!((read - length).abs() < 1e-6, "{text} read as {read}");
}

/// What a length tag might say: hours, minutes and seconds of any number,
/// or their characters any which way.
#[hegel::composite]
fn length_text(tc: &TestCase) -> String {
    tc.draw(hegel::one_of!(
        hegel::tuples!(
            gs::integers::<i64>(),
            gs::integers::<i64>(),
            gs::floats::<f64>()
        )
        .map(|(hours, minutes, seconds)| format!("{hours}:{minutes}:{seconds}")),
        gs::from_regex("[ 0-9]{0,4}(:[-+ 0-9.eEinfINFa]{0,10}){0,4}"),
    ))
}

#[hegel::test]
fn any_length_read_is_a_time(tc: TestCase) {
    let text = tc.draw(length_text());
    if let Some(length) = length_in_seconds(&text) {
        assert!(
            length.is_finite() && length >= 0.0,
            "{text} read as {length}"
        );
    }
}
