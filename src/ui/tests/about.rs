use super::*;

#[test]
fn spells_the_build_date() {
    assert_eq!(spell_date("2026-11-18"), "18 November 2026");
    assert_eq!(spell_date("2027-01-02"), "2 January 2027");
    assert_eq!(spell_date("soon"), "soon");
    assert!(release_date().ends_with(char::is_numeric));
}
