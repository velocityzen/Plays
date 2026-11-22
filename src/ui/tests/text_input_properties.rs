use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn any_utf16_offset_lands_on_a_character_and_each_goes_there_and_back(
        text in any::<String>(),
        offset in 0usize..64,
    ) {
        let byte = utf8_offset(&text, offset);
        prop_assert!(text.is_char_boundary(byte));
        prop_assert!(utf16_offset(&text, byte) >= offset.min(text.encode_utf16().count()));
        for (start, _) in text.char_indices() {
            prop_assert_eq!(utf8_offset(&text, utf16_offset(&text, start)), start);
        }
    }

    #[test]
    fn any_range_slices_the_text(text in any::<String>(), start in 0usize..64, end in 0usize..64) {
        let range = within(&text, start..end);
        prop_assert!(range.start <= range.end);
        let _ = &text[range];
        let _ = &text[utf8_range(&text, &(start..end))];
    }
}
