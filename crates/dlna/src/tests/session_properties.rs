use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn a_subscription_lasts_within_bounds_whatever_its_timeout(
        timeout in prop_oneof![
            any::<u64>().prop_map(|seconds| format!("Second-{seconds}")),
            "[sS][eE][cC][oO][nN][dD]-[0-9]{1,40}",
            "[ ]?[iI][nN][fF][iI][nN][iI][tT][eE][ ]?",
            any::<String>(),
        ],
    ) {
        let lasts = subscribed_for(Some(&timeout));
        prop_assert!((SHORTEST_SUBSCRIPTION..=LONGEST_SUBSCRIPTION).contains(&lasts));
    }

    #[test]
    fn a_subscription_lasts_as_long_as_it_was_granted_within_bounds(
        seconds in prop_oneof![0..=2 * LONGEST_SUBSCRIPTION.as_secs(), any::<u64>()],
    ) {
        prop_assert_eq!(
            subscribed_for(Some(&format!("Second-{seconds}"))),
            Duration::from_secs(seconds).clamp(SHORTEST_SUBSCRIPTION, LONGEST_SUBSCRIPTION)
        );
    }
}
