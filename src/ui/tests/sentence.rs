use super::*;

#[test]
fn begins_with_a_capital() {
    assert_eq!(
        capitalized("pairing failed: the PIN is wrong"),
        "Pairing failed: the PIN is wrong"
    );
    assert_eq!(capitalized("éte"), "Éte");
    assert_eq!(capitalized(""), "");
}
