use super::*;

#[test]
fn beside_one_is_after_or_before_it_once_the_mover_has_left_its_place() {
    let at = |number: u64, id: &str| json!({"id": id, "number": number});
    let place = |from, to, after| beside(&at(from, "a"), &at(to, "b"), after, "block").unwrap();
    // 1 2 3 4: 1 after 3 is 2 3 1 4, at 3; before 3, 2 1 3 4, at 2.
    assert_eq!(place(1, 3, true), 3);
    assert_eq!(place(1, 3, false), 2);
    // 4 after 2 is 1 2 4 3, at 3; before 2, 1 4 2 3, at 2.
    assert_eq!(place(4, 2, true), 3);
    assert_eq!(place(4, 2, false), 2);
    let itself = beside(&at(1, "a"), &at(1, "a"), true, "block").unwrap_err();
    assert_eq!(itself.to_string(), "a block can't move beside itself");
}

#[test]
fn a_blocks_state_is_its_files_opening_or_failing_before_its_playing() {
    let block = |opened, error: Option<&str>, playing| json!({"opened": opened, "error": error, "playing": playing});
    assert_eq!(state(&block(false, None, false)), "opening");
    // Set to play as it opens.
    assert_eq!(state(&block(false, None, true)), "opening");
    assert_eq!(state(&block(false, Some("no such file"), false)), "failed");
    assert_eq!(state(&block(true, Some("damaged"), true)), "failed");
    assert_eq!(state(&block(true, None, true)), "playing");
    assert_eq!(state(&block(true, None, false)), "paused");
    // From a Plays that doesn't say.
    assert_eq!(state(&json!({"playing": true})), "playing");
}
