use super::*;

/// A `width` x `height` frame of 4:2:0 Y'CbCr, each sample telling
/// where it is.
fn pattern(width: u32, height: u32) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::YUV420P, width, height);
    for plane in 0..3 {
        let (data, stride) = ffmpeg_safe::plane_mut(&mut frame, plane).unwrap();
        for (y, row) in data.chunks_mut(stride).enumerate() {
            for (x, sample) in row.iter_mut().enumerate() {
                *sample = (x * 3 + y * 5 + plane * 64) as u8;
            }
        }
    }
    frame
}

#[test]
fn sdr_is_scaled_straight_into_the_pictures_pixels() {
    ffmpeg::init().unwrap();
    // 162 is not a multiple of 4: rows that don't end on 16 bytes.
    for (width, height) in [(160, 90), (162, 91), (2, 2)] {
        let frame = pattern(width, height);
        let mut scaler = None;
        let (w, h, pixels) = convert_pixels(&mut scaler, None, &frame).unwrap();
        assert_eq!((w, h), (width, height & !1));

        // As swscale makes it in a frame of its own.
        let mut context = scaling::Context::get(
            Pixel::YUV420P,
            width,
            height,
            Pixel::BGRA,
            w,
            h,
            scaling::Flags::BILINEAR,
        )
        .unwrap();
        set_colorspace(&mut context, &frame);
        let mut own = frame::Video::empty();
        context.run(&frame, &mut own).unwrap();
        let row = w as usize * 4;
        let (data, stride) = ffmpeg_safe::plane(&own, 0).unwrap();
        let expected: Vec<u8> = data
            .chunks(stride)
            .take(h as usize)
            .flat_map(|line| &line[..row])
            .copied()
            .collect();
        assert!(pixels == expected, "{width}x{height}: other pixels");
        assert_eq!(
            scaler.unwrap().output.format(),
            Pixel::None,
            "{width}x{height}: a frame in between"
        );
    }
}

/// HDR10 comes out tone mapped to the peak its metadata says.
#[test]
fn hdr10_is_tone_mapped_to_its_peak() {
    ffmpeg::init().unwrap();
    // Grey at about 300 nits all over, in a video that peaks at 1,000:
    // where the curve fitted to it and the one not are furthest apart.
    let luma: u16 = 610;
    let mut frame = frame::Video::new(Pixel::YUV420P10LE, 16, 16);
    frame.set_color_space(color::Space::BT2020NCL);
    frame.set_color_primaries(color::Primaries::BT2020);
    frame.set_color_transfer_characteristic(color::TransferCharacteristic::SMPTE2084);
    frame.set_color_range(color::Range::MPEG);
    for (plane, code) in [(0, luma), (1, 512), (2, 512)] {
        for pair in ffmpeg_safe::plane_mut(&mut frame, plane)
            .unwrap()
            .0
            .as_chunks_mut::<2>()
            .0
        {
            *pair = code.to_le_bytes();
        }
    }
    hdr::attach_peak(&mut frame, 1000.0).unwrap();
    let (_, _, pixels) = convert_pixels(&mut None, None, &frame).unwrap();
    let mapped = |peak| {
        let mut mapper = hdr::ToneMapper::new(hdr::Transfer::Pq);
        mapper.set_gamma(picture::gamma(true));
        mapper.set_peak(peak);
        let light = mapper.pq_light(f32::from(luma - 64) / 876.0);
        mapper.output([light; 3])
    };
    let (fitted, unfitted) = (mapped(Some(1000.0)), mapped(None));
    let apart = |a: [u8; 4], b: [u8; 4]| (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
    assert!(apart(fitted, unfitted) > 3, "{fitted:?} and {unfitted:?}");
    for &pixel in pixels.as_chunks::<4>().0 {
        assert!(apart(pixel, fitted) <= 1, "{pixel:?}, not {fitted:?}");
    }
}

/// Dolby Vision comes out as its reshaping and a tone mapper fitted to the
/// peak of the display it was mastered on make it.
#[test]
fn dolby_vision_is_tone_mapped_to_its_mastering_display() {
    use crate::media::dovi::tests::rpu_frame;

    // A bright IPT color all over, mastered at 1,000 nits.
    let codes = [922, 461, 461];
    let frame = rpu_frame((16, 16), codes, 3079);
    let reshaping = dovi::Reshaping::of(&frame).unwrap();
    let (_, _, pixels) = convert_pixels(&mut None, None, &frame).unwrap();
    let mapped = |peak| {
        let mut mapper = hdr::ToneMapper::new(hdr::Transfer::Pq);
        mapper.set_gamma(picture::gamma(true));
        mapper.set_peak(peak);
        let samples = codes.map(|code| f32::from(code << 6) / 65536.0);
        let lms = reshaping.lms_signal(samples).map(|s| mapper.pq_light(s));
        mapper.output(reshaping.lms_to_rgb(lms))
    };
    let (fitted, unfitted) = (mapped(reshaping.peak_nits()), mapped(None));
    let apart = |a: [u8; 4], b: [u8; 4]| (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
    assert!(apart(fitted, unfitted) > 3, "{fitted:?} and {unfitted:?}");
    for &pixel in pixels.as_chunks::<4>().0 {
        assert!(apart(pixel, fitted) <= 1, "{pixel:?}, not {fitted:?}");
    }
}
