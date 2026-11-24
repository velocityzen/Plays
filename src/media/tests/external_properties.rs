use hegel::TestCase;
use hegel::generators as gs;

use super::*;

#[hegel::test]
fn any_bytes_but_utf8_are_named_an_encoding(tc: TestCase) {
    let bytes = tc.draw(gs::binary());
    let unicode =
        std::str::from_utf8(&bytes).is_ok() || encoding_rs::Encoding::for_bom(&bytes).is_some();
    assert_eq!(encoding(&bytes, true).is_none(), unicode);
}

#[hegel::test]
fn utf8_cut_short_by_the_sample_is_utf8(tc: TestCase) {
    let text = tc.draw(gs::text());
    let cut = tc.draw(gs::integers::<usize>().max_value(3));
    let bytes = &text.as_bytes()[..text.len().saturating_sub(cut)];
    assert_eq!(encoding(bytes, false), None);
}
