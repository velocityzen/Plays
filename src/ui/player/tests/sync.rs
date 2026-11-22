use super::*;

#[test]
fn says_delays_plainly() {
    assert_eq!(delay_text(0.0), "0 ms");
    assert_eq!(delay_text(-0.0), "0 ms");
    assert_eq!(delay_text(0.2), "+200 ms");
    assert_eq!(delay_text(-0.05), "−50 ms");
    assert_eq!(delay_text(1.0), "+1 s");
    assert_eq!(delay_text(-1.25), "−1.25 s");
    assert_eq!(delay_text(12.3), "+12.3 s");
    // Steps of 50 ms add up without drifting.
    let delay = (0..7).fold(0.0, |d, _| to_milliseconds(d + 0.05));
    assert_eq!(delay_text(delay), "+350 ms");
    assert_eq!(to_milliseconds(delay), 0.35);
}
