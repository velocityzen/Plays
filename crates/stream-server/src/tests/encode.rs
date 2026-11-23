use super::*;

/// An encoder failing to open (too big a picture, or the hardware's
/// sessions used up) is left alone a while at that size, not for good.
#[test]
fn tries_an_encoder_again_after_it_fails_to_open() {
    let mut probed = Probed::default();
    let start = Instant::now();
    let first = video_encoders(VideoCodec::H264)[0];
    let never = |_: &'static str| -> bool { panic!("tried again") };

    // Too big for it, so none here at that size, for a while.
    let big = (8192, 4320);
    assert_eq!(
        probed.encoder(VideoCodec::H264, big, false, start, |_| false),
        None
    );
    let later = start + Duration::from_secs(10);
    assert_eq!(
        probed.encoder(VideoCodec::H264, big, false, later, never),
        None
    );
    // A size it takes is tried, and finds it; from then on it's had.
    let hd = (1920, 1080);
    assert_eq!(
        probed.encoder(VideoCodec::H264, hd, false, later, |_| true),
        Some(first)
    );
    assert_eq!(
        probed.encoder(VideoCodec::H264, (640, 360), false, later, never),
        Some(first)
    );

    // Busy at first, and free a minute on.
    let first = video_encoders(VideoCodec::Hevc)[0];
    assert_eq!(
        probed.encoder(VideoCodec::Hevc, hd, true, start, |_| false),
        None
    );
    let soon = start + Duration::from_secs(30);
    assert_eq!(
        probed.encoder(VideoCodec::Hevc, hd, true, soon, never),
        None
    );
    let minute = start + RETRY_AFTER;
    assert_eq!(
        probed.encoder(VideoCodec::Hevc, hd, true, minute, |_| true),
        Some(first)
    );
}

/// Sound whose channels are in an uncommon order, the right before the
/// left, goes through one resampler from first to last, all of it, each
/// frame's layout freed with it as a decoder's is.
#[test]
fn keeps_one_resampler_for_channels_in_an_uncommon_order() {
    crate::init();
    let mut encoder = AudioEncoder::open(&AudioSettings {
        codec: AudioCodec::Aac,
        channels: 2,
        sample_rate: 48_000,
        bit_rate: 128_000,
        global_header: false,
    })
    .unwrap();
    let mut encoded = Packet::empty();
    let mut made = None;
    for at in 0..10 {
        let mut decoded = frame::Audio::new(
            Sample::I16(ffmpeg::format::sample::Type::Packed),
            480,
            ChannelLayout::STEREO,
        );
        decoded.set_rate(48_000);
        decoded.data_mut(0).fill(0);
        // SAFETY: the frame is valid, and its layout its own to replace.
        unsafe {
            let layout = &mut (*decoded.as_mut_ptr()).ch_layout;
            ffi::av_channel_layout_uninit(layout);
            let name = c"FR+FL";
            assert_eq!(ffi::av_channel_layout_from_string(layout, name.as_ptr()), 0);
        }
        encoder
            .send(&mut decoded, f64::from(at) * 0.01, 0.0)
            .unwrap();
        while encoder.receive(&mut encoded) {}
        // SAFETY: only the address is compared.
        let resampler = encoder
            .resampler
            .as_ref()
            .map(|(r, _)| unsafe { r.as_ptr() });
        assert_eq!(*made.get_or_insert(resampler), resampler, "made again");
    }
    let reached = encoder.reached();
    assert!((reached - 0.1).abs() < 0.005, "reached {reached} s");
}
