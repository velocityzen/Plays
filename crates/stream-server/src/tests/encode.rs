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

/// A stereo AAC encoder at 48 kHz.
fn aac() -> AudioEncoder {
    crate::init();
    AudioEncoder::open(&AudioSettings {
        codec: AudioCodec::Aac,
        channels: 2,
        sample_rate: 48_000,
        bit_rate: 128_000,
        global_header: false,
    })
    .unwrap()
}

/// Sound whose channels are in an uncommon order, the right before the
/// left, goes through one resampler from first to last, all of it, each
/// frame's layout freed with it as a decoder's is.
#[test]
fn keeps_one_resampler_for_channels_in_an_uncommon_order() {
    let mut encoder = aac();
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

/// A decoded frame holding many of the encoder's, as a FLAC block or PCM
/// does, is encoded whole, in order: the fixture's 1024 samples at 8 kHz
/// are six frames of AAC at 48 kHz.
#[test]
fn encodes_frames_holding_many_of_its_own() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/uncommon-order.mov");
    let mut input = ffmpeg::format::input(&path).unwrap();
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .expect("sound");
    let (index, time_base) = (stream.index(), f64::from(stream.time_base()));
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())
        .unwrap()
        .decoder()
        .audio()
        .unwrap();
    let mut encoder = aac();
    let (mut decoded, mut encoded) = (frame::Audio::empty(), Packet::empty());
    let mut times = Vec::new();
    for (stream, packet) in input.packets() {
        if stream.index() != index {
            continue;
        }
        decoder.send_packet(&packet).unwrap();
        while decoder.receive_frame(&mut decoded).is_ok() {
            let time = decoded.timestamp().unwrap() as f64 * time_base;
            encoder.send(&mut decoded, time, 0.0).unwrap();
            while encoder.receive(&mut encoded) {
                times.push(encoded.pts().unwrap());
            }
        }
    }
    encoder.send_eof().unwrap();
    while encoder.receive(&mut encoded) {
        times.push(encoded.pts().unwrap());
    }
    assert!(times.is_sorted(), "{times:?}");
    let seconds = times.len() as f64 * 1024.0 / 48_000.0;
    assert!((seconds - 0.5).abs() < 0.05, "{seconds} s encoded");
}
