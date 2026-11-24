use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;

#[hegel::test]
fn a_subscription_lasts_within_bounds_whatever_its_timeout(tc: TestCase) {
    let timeout = tc.draw(hegel::one_of!(
        gs::integers::<u64>().map(|seconds| format!("Second-{seconds}")),
        gs::from_regex("[sS][eE][cC][oO][nN][dD]-[0-9]{1,40}"),
        gs::from_regex("[ ]?[iI][nN][fF][iI][nN][iI][tT][eE][ ]?"),
        gs::text(),
    ));
    let lasts = subscribed_for(Some(&timeout));
    assert!((SHORTEST_SUBSCRIPTION..=LONGEST_SUBSCRIPTION).contains(&lasts));
}

#[hegel::test]
fn a_subscription_lasts_as_long_as_it_was_granted_within_bounds(tc: TestCase) {
    let seconds = tc.draw(hegel::one_of!(
        gs::integers::<u64>().max_value(2 * LONGEST_SUBSCRIPTION.as_secs()),
        gs::integers::<u64>(),
    ));
    assert_eq!(
        subscribed_for(Some(&format!("Second-{seconds}"))),
        Duration::from_secs(seconds).clamp(SHORTEST_SUBSCRIPTION, LONGEST_SUBSCRIPTION)
    );
}
