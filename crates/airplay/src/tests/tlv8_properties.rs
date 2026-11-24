use proptest::collection::vec;
use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn values_come_back_whole_however_long(
        items in vec((any::<u8>(), vec(any::<u8>(), 0..600)), 0..6),
    ) {
        let tlv = Tlv { items };
        prop_assert_eq!(Tlv::decode(&tlv.encode()).unwrap(), tlv);
    }

    #[test]
    fn any_bytes_are_read_or_refused_without_panicking(bytes in vec(any::<u8>(), 0..1024)) {
        if let Ok(tlv) = Tlv::decode(&bytes) {
            // Each piece is its tag, its length and its value.
            let held: usize = tlv.items.iter().map(|(_, value)| 2 + value.len()).sum();
            prop_assert!(held <= bytes.len());
        }
    }
}
