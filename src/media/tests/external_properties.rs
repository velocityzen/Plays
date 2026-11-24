use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn any_bytes_but_utf8_are_named_an_encoding(bytes in any::<Vec<u8>>()) {
        let unicode = std::str::from_utf8(&bytes).is_ok()
            || encoding_rs::Encoding::for_bom(&bytes).is_some();
        prop_assert_eq!(encoding(&bytes, true).is_none(), unicode);
    }

    #[test]
    fn utf8_cut_short_by_the_sample_is_utf8(text in any::<String>(), cut in 0usize..4) {
        let bytes = &text.as_bytes()[..text.len().saturating_sub(cut)];
        prop_assert_eq!(encoding(bytes, false), None);
    }
}
