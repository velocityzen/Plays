use super::*;

#[test]
fn reads_time_seeks() {
    assert_eq!(npt_start("npt=12.5-"), Some(12.5));
    assert_eq!(npt_start("npt=0:01:02.5-30"), Some(62.5));
    assert_eq!(npt_start("npt=-"), Some(0.0));
    assert_eq!(npt_start("bytes=0-"), None);
    assert_eq!(npt_start("npt=abc-"), None);
}

#[test]
fn names_segments() {
    assert_eq!(segment_number("12.ts", Segments::Ts), Some(12));
    assert_eq!(segment_number("3.m4s", Segments::Fmp4), Some(3));
    assert_eq!(segment_number("3.ts", Segments::Fmp4), None);
    assert_eq!(segment_number("x.ts", Segments::Ts), None);
}
