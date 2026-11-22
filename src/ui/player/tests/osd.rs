use super::{REEL_FADED, reel_opacity};

#[test]
fn fades_the_reel_away_from_the_middle() {
    let opacity = |distance, count| (reel_opacity(distance, count) * 100.0).round() / 100.0;
    assert_eq!(opacity(0.0, 5), 1.0);
    assert_eq!(opacity(-1.0, 5), REEL_FADED);
    assert_eq!(opacity(1.0, 5), REEL_FADED);
    assert_eq!(opacity(0.5, 5), 0.7);
    assert_eq!(opacity(1.5, 5), 0.0);
    assert_eq!(opacity(-2.0, 5), 0.0);
    // Two tracks: the other one shows below only.
    assert_eq!(opacity(1.0, 2), REEL_FADED);
    assert_eq!(opacity(-1.0, 2), 0.0);
    assert_eq!(opacity(0.0, 1), 1.0);
    assert_eq!(opacity(1.0, 1), 0.0);
}
