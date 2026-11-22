use super::*;

#[test]
fn numbers_ffmpeg_has_no_names_for_are_unspecified() {
    let odd = Colors::named(3, 30, 15, 40);
    assert_eq!(odd.range, color::Range::Unspecified);
    assert_eq!(odd.space, color::Space::Unspecified);
    assert_eq!(odd.primaries, color::Primaries::Unspecified);
    assert_eq!(odd.transfer, color::TransferCharacteristic::Unspecified);
    let hdr10 = Colors::named(1, 9, 9, 16);
    assert_eq!(hdr10.range, color::Range::MPEG);
    assert_eq!(hdr10.space, color::Space::BT2020NCL);
    assert_eq!(hdr10.primaries, color::Primaries::BT2020);
    assert_eq!(hdr10.transfer, color::TransferCharacteristic::SMPTE2084);
}
