use super::*;

#[test]
fn a_long_value_goes_in_pieces_and_comes_back_whole() {
    let key: Vec<u8> = (0..384).map(|i| i as u8).collect();
    let tlv = Tlv::new()
        .byte(Tag::State, 3)
        .with(Tag::PublicKey, &key)
        .with(Tag::Proof, [7; 64]);
    let bytes = tlv.encode();
    assert_eq!(&bytes[..3], [0x06, 1, 3]);
    assert_eq!(&bytes[3..5], [0x03, 255]);
    assert_eq!(&bytes[260..262], [0x03, 129]);
    assert_eq!(bytes.len(), 3 + 2 + 255 + 2 + 129 + 2 + 64);
    let back = Tlv::decode(&bytes).unwrap();
    assert_eq!(back, tlv);
    assert_eq!(back.get(Tag::PublicKey), Some(key.as_slice()));
    assert_eq!(back.state(), Some(3));
}

#[test]
fn a_value_of_exactly_255_bytes_isnt_joined_to_the_next_item_of_another_kind() {
    let tlv = Tlv::new()
        .with(Tag::Salt, [1; 255])
        .with(Tag::PublicKey, [2; 3]);
    assert_eq!(Tlv::decode(&tlv.encode()).unwrap(), tlv);
}

#[test]
fn a_message_cut_short_is_an_error() {
    assert!(Tlv::decode(&[0x06, 2, 1]).is_err());
    assert!(Tlv::decode(&[0x06]).is_err());
}
