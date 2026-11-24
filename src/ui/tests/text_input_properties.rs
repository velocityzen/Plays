use hegel::TestCase;
use hegel::generators as gs;

use super::*;

#[hegel::test]
fn any_utf16_offset_lands_on_a_character_and_each_goes_there_and_back(tc: TestCase) {
    let text = tc.draw(gs::text());
    let offset = tc.draw(gs::integers::<usize>().max_value(63));
    let byte = utf8_offset(&text, offset);
    assert!(text.is_char_boundary(byte));
    assert!(utf16_offset(&text, byte) >= offset.min(text.encode_utf16().count()));
    for (start, _) in text.char_indices() {
        assert_eq!(utf8_offset(&text, utf16_offset(&text, start)), start);
    }
}

#[hegel::test]
fn any_range_slices_the_text(tc: TestCase) {
    let text = tc.draw(gs::text());
    let start = tc.draw(gs::integers::<usize>().max_value(63));
    let end = tc.draw(gs::integers::<usize>().max_value(63));
    let range = within(&text, start..end);
    assert!(range.start <= range.end);
    let _ = &text[range];
    let _ = &text[utf8_range(&text, &(start..end))];
}
