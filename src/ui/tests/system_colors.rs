use super::palette;

#[test]
fn reads_both_appearances() {
    let dark = palette(true).expect("dark colors");
    let light = palette(false).expect("light colors");
    assert!(dark.background.l < light.background.l);
    assert!(dark.text.l > light.text.l);
    assert!(dark.accent.a > 0.9 && light.accent.a > 0.9);
}
