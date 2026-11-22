use super::*;

#[test]
fn a_sender_made_afresh_has_an_id_of_its_own_like_a_mac_address() {
    let (one, other) = (Sender::new("One"), Sender::default());
    assert_ne!(one.id, other.id);
    for id in [one.id, other.id] {
        let bytes: Vec<u8> = id
            .split(':')
            .map(|byte| {
                assert_eq!(byte.len(), 2, "{id}");
                assert_eq!(byte, byte.to_uppercase(), "{id}");
                u8::from_str_radix(byte, 16).unwrap()
            })
            .collect();
        assert_eq!(bytes.len(), 6, "{id}");
        assert_eq!(bytes[0] & 0x03, 0x02, "made up, for one device: {id}");
    }
    assert_eq!(other.name, "AirPlay");
}
