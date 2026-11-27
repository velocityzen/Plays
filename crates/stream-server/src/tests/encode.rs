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
    let mut input = ffmpeg_safe::input_file(&path).unwrap();
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

/// A hardware encoder at 32 × 16, 10-bit HEVC or 8-bit H.264, tagged with
/// `colors`, else BT.709, reshaping Dolby Vision's IPT if it `reshapes`.
fn video_encoder(ten_bit: bool, colors: Option<Colors>, reshapes: bool) -> VideoEncoder {
    crate::init();
    let codec = if ten_bit {
        VideoCodec::Hevc
    } else {
        VideoCodec::H264
    };
    let encoder = super::video_encoder(codec, 32, 16, ten_bit).expect("a hardware encoder");
    VideoEncoder::open(&VideoSettings {
        encoder,
        codec,
        width: 32,
        height: 16,
        ten_bit,
        bit_rate: 1_000_000,
        time_base: Rational::new(1, 25),
        frame_rate: Some(Rational::new(25, 1)),
        colors,
        reshapes,
        global_header: false,
    })
    .expect("the encoder opens")
}

/// HDR going into an encoding tagged BT.709 is fitted to SDR, at either
/// depth, so its reference white shows nearly as SDR's white, not as the
/// dim grey PQ's code for it is taken for BT.709's. An encoding keeping
/// HDR's colors gets its codes as they are, and IPT (Dolby Vision's
/// profile 20) is only scaled.
#[cfg(target_os = "macos")]
#[test]
fn fits_hdr_to_sdr_for_an_encoding_tagged_bt709() {
    use crate::hdr::tests::{grey, luma, pq};
    use ffmpeg::color::{Space, TransferCharacteristic};
    let white = pq(203.0);
    let pq_white = grey(TransferCharacteristic::SMPTE2084, Space::BT2020NCL, white);
    let mut ipt = pq_white.clone();
    // SAFETY: the frame is ours, and IPT-C2 a value FFmpeg names.
    unsafe { (*ipt.as_mut_ptr()).colorspace = ffi::AVColorSpace::AVCOL_SPC_IPT_C2 };

    for ten_bit in [false, true] {
        let mut sdr = video_encoder(ten_bit, None, false);
        let fitted = sdr.scale(&pq_white).unwrap();
        assert!(luma(&fitted) > 0.9, "{ten_bit}: {}", luma(&fitted));
        let scaled = sdr.scale(&ipt).unwrap();
        assert!((luma(&scaled) - white).abs() < 0.01, "{ten_bit}");
    }
    let mut kept = video_encoder(true, Some(Colors::HDR10), false);
    let scaled = kept.scale(&pq_white).unwrap();
    assert!((luma(&scaled) - white).abs() < 0.01, "{}", luma(&scaled));
}

/// Dolby Vision's IPT is reshaped by its RPU: into HDR10 for an encoding
/// keeping HDR10's colors, so its reference white has PQ's code for 203
/// nits, and fitted to SDR otherwise, as HDR10 is, so it shows nearly as
/// SDR's white. Grey stays grey either way.
#[cfg(target_os = "macos")]
#[test]
fn reshapes_dolby_visions_ipt_into_hdr10_or_sdr() {
    use ::hdr::testing::NEUTRAL;

    use crate::hdr::TV_GAMMA;
    use crate::hdr::tests::{luma, pq, rpu_frame, sdr_tags};
    use ffmpeg_safe::Tags;
    let neutral = (NEUTRAL * 1024.0).round() as u16;
    // The luma code this frame's curve, x² / 2 + 1/4 there, reshapes to
    // PQ's code for 203 nits.
    let white = rpu_frame([833, neutral, neutral]);
    let chroma = |frame: &ffmpeg::frame::Video| {
        let (data, stride) = ffmpeg_safe::plane(frame, 1).unwrap();
        let at = stride * 4 + 16;
        [0, 2].map(|i| u16::from_le_bytes([data[at + i], data[at + i + 1]]) >> 6)
    };

    let mut hdr10 = video_encoder(true, Some(Colors::HDR10), true);
    let reshaped = hdr10.scale(&white).unwrap();
    assert!(
        (luma(&reshaped) - pq(203.0)).abs() < 0.005,
        "{}",
        luma(&reshaped)
    );
    assert!(chroma(&reshaped).iter().all(|c| c.abs_diff(512) <= 1));
    assert_eq!(
        Tags::of_frame(&reshaped),
        Tags {
            primaries: ffi::AVColorPrimaries::AVCOL_PRI_BT2020 as u32,
            transfer: ffi::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE2084 as u32,
            space: ffi::AVColorSpace::AVCOL_SPC_BT2020_NCL as u32,
            range: ffi::AVColorRange::AVCOL_RANGE_MPEG as u32,
        }
    );

    for ten_bit in [false, true] {
        let mut sdr = video_encoder(ten_bit, None, true);
        let fitted = sdr.scale(&white).unwrap();
        let light = luma(&fitted).powf(f64::from(TV_GAMMA));
        assert!((0.8..0.95).contains(&light), "{ten_bit}: {light}");
        assert_eq!(Tags::of_frame(&fitted), sdr_tags());
    }
    // Not reshaped, its chroma stays IPT's, which YCbCr shows green.
    let mut scaled = video_encoder(true, Some(Colors::HDR10), false);
    let scaled = scaled.scale(&white).unwrap();
    assert!(chroma(&scaled).iter().all(|c| c.abs_diff(neutral) <= 1));
}

/// A 10-bit encoding keeps the source's colors unless they're HDR the
/// device isn't given; an 8-bit one never does.
#[test]
fn keeps_the_sources_colors_only_as_the_device_shows_them() {
    use ffi::AVColorTransferCharacteristic::*;
    let colors = |transfer| Colors {
        primaries: Some(ffi::AVColorPrimaries::AVCOL_PRI_BT2020),
        transfer: Some(transfer),
        space: Some(ffi::AVColorSpace::AVCOL_SPC_BT2020_NCL),
    };
    let (pq, hlg, sdr) = (
        colors(AVCOL_TRC_SMPTE2084),
        colors(AVCOL_TRC_ARIB_STD_B67),
        colors(AVCOL_TRC_BT2020_10),
    );
    let kept = |colors, ten_bit, hdr| kept_colors(colors, ten_bit, hdr, false).is_some();
    assert!(kept(pq, true, Some(Hdr::Hdr10)));
    assert!(kept(hlg, true, Some(Hdr::Hlg)));
    assert!(!kept(pq, true, None));
    assert!(!kept(hlg, true, None));
    assert!(kept(sdr, true, None));
    assert!(kept(Colors::default(), true, None));
    assert!(!kept(pq, false, Some(Hdr::Hdr10)));
    assert!(!kept(sdr, false, None));
    // Dolby Vision's IPT, whatever it's tagged, is reshaped into HDR10.
    for colors in [pq, Colors::default()] {
        let reshaped = |ten_bit, hdr| kept_colors(colors, ten_bit, hdr, true);
        assert!(matches!(
            reshaped(true, Some(Hdr::Hdr10)),
            Some(Colors {
                space: Some(ffi::AVColorSpace::AVCOL_SPC_BT2020_NCL),
                ..
            })
        ));
        assert!(reshaped(true, None).is_none());
        assert!(reshaped(false, Some(Hdr::Hdr10)).is_none());
    }
}
