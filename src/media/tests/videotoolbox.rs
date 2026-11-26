use super::*;
use crate::media::dovi::tests::rpu_frame;
use crate::media::tests::attached_peak;

/// A grey 10-bit BT.2020 PQ frame.
fn pq_frame() -> frame::Video {
    let mut frame = frame::Video::new(Pixel::P010LE, 16, 16);
    for plane in 0..2 {
        frame.data_mut(plane).fill(0x80);
    }
    frame.set_color_range(color::Range::MPEG);
    frame.set_color_space(color::Space::BT2020NCL);
    frame.set_color_primaries(color::Primaries::BT2020);
    frame.set_color_transfer_characteristic(color::TransferCharacteristic::SMPTE2084);
    frame
}

#[test]
fn a_surface_carries_its_videos_peak_and_gives_it_back_read() {
    let mut converter = Converter::new().unwrap();
    let mut mastered = pq_frame();
    hdr::attach_peak(&mut mastered, 1000.0).unwrap();
    let surface = converter.upload(&mastered, 16, 16).unwrap();
    assert_eq!(attached_peak(surface.buffer()), Some(1000.0));
    assert_eq!(hdr::peak_nits(&surface.read().unwrap()), Some(1000.0));

    let unsaid = converter.upload(&pq_frame(), 16, 16).unwrap();
    assert_eq!(attached_peak(unsaid.buffer()), None);
    assert_eq!(hdr::peak_nits(&unsaid.read().unwrap()), None);

    // Dolby Vision's, from its RPU.
    let surface = converter
        .upload(&rpu_frame((16, 16), [512; 3], 3079), 16, 16)
        .unwrap();
    let peak = attached_peak(surface.buffer()).unwrap();
    assert!((peak - 1000.6).abs() < 0.1, "{peak}");
}
