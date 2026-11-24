use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// A tag, often one of two, so items of a tag come in a row.
#[hegel::composite]
fn tag(tc: &TestCase) -> u8 {
    tc.draw(hegel::one_of!(
        gs::sampled_from(vec![0x01u8, 0x06]),
        gs::integers::<u8>()
    ))
}

/// A value up to 600 bytes long, often about where it splits into pieces.
#[hegel::composite]
fn value(tc: &TestCase) -> Vec<u8> {
    let length = tc.draw(hegel::one_of!(
        gs::sampled_from(vec![0usize, 1, 254, 255, 256, 509, 510, 511]),
        gs::integers::<usize>().max_value(599),
    ));
    tc.draw(gs::binary().min_size(length).max_size(length))
}

#[hegel::test]
fn values_come_back_whole_however_long(tc: TestCase) {
    let items = tc.draw(gs::vecs(hegel::tuples!(tag(), value())).max_size(5));
    let tlv = Tlv { items };
    assert_eq!(Tlv::decode(&tlv.encode()).unwrap(), tlv);
}

#[hegel::test]
fn any_bytes_are_read_or_refused_without_panicking(tc: TestCase) {
    let bytes = tc.draw(gs::binary().max_size(1023));
    if let Ok(tlv) = Tlv::decode(&bytes) {
        // Each piece is its tag, its length and its value.
        let held: usize = tlv.items.iter().map(|(_, value)| 2 + value.len()).sum();
        assert!(held <= bytes.len());
    }
}
