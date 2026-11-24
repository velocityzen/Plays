use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// Bytes laid out as TLV8, or nearly: pieces of a few tags, so pieces of
/// a tag follow each other, of lengths about where a value splits, with
/// any bytes between them, cut short or not.
fn wire(tc: &TestCase) -> Vec<u8> {
    let mut wire = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        if tc.draw(gs::weighted_booleans(0.9)) {
            let tag = tc.draw(gs::sampled_from(vec![0x01u8, 0x06, 0x0A, 0xFF]));
            let length = tc.draw(hegel::one_of!(
                gs::sampled_from(vec![0u8, 1, 254, 255]),
                gs::integers::<u8>()
            ));
            wire.extend([tag, length]);
            let length = usize::from(length);
            wire.extend(tc.draw(gs::binary().min_size(length).max_size(length)));
        } else {
            wire.extend(tc.draw(gs::binary().max_size(4)));
        }
    }
    if tc.draw(gs::weighted_booleans(0.2)) {
        wire.truncate(tc.draw(gs::integers::<usize>().max_value(wire.len())));
    }
    wire
}

#[hegel::test]
fn what_any_bytes_read_as_writes_back_the_same(tc: TestCase) {
    let wire = wire(&tc);
    if let Ok(tlv) = Tlv::decode(&wire) {
        assert_eq!(Tlv::decode(&tlv.encode()).unwrap(), tlv);
    }
}
