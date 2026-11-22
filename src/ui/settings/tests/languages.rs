use super::*;

fn list(codes: &[&str]) -> Vec<String> {
    codes.iter().map(|&code| code.to_owned()).collect()
}

#[test]
fn a_language_moves_up_or_down_one() {
    let chosen = list(&["eng", "fre", "ger"]);
    assert_eq!(moved(&chosen, 1, 0), list(&["fre", "eng", "ger"]));
    assert_eq!(moved(&chosen, 1, 2), list(&["eng", "ger", "fre"]));
    assert_eq!(moved(&chosen, 0, 0), chosen, "the first stays");
    assert_eq!(moved(&chosen, 2, 2), chosen, "and the last");
}
