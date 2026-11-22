use super::*;

#[test]
fn why_a_pin_didnt_pair_begins_as_a_sentence() {
    assert_eq!(
        sentence("pairing failed: the PIN is wrong"),
        "Pairing failed: the PIN is wrong"
    );
    assert_eq!(sentence(""), "");
}
