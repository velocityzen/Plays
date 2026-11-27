use super::*;
use crate::media::dovi::tests::rpu_frame;
use crate::media::tests::attached_peak;
use crate::media::yuv::tests::stored_bottom_up;

/// A grey 10-bit BT.2020 PQ frame.
fn pq_frame() -> frame::Video {
    let mut frame = frame::Video::new(Pixel::P010LE, 16, 16);
    for plane in 0..2 {
        ffmpeg_safe::plane_mut(&mut frame, plane)
            .unwrap()
            .0
            .fill(0x80);
    }
    tag_pq(&mut frame);
    frame
}

/// Tag `frame` as video range BT.2020 PQ.
fn tag_pq(frame: &mut frame::Video) {
    frame.set_color_range(color::Range::MPEG);
    frame.set_color_space(color::Space::BT2020NCL);
    frame.set_color_primaries(color::Primaries::BT2020);
    frame.set_color_transfer_characteristic(color::TransferCharacteristic::SMPTE2084);
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

/// A 10-bit 4:2:0 frame of `width` x `height`, every sample different,
/// tagged as `pq_frame` is.
fn planar_frame(width: u32, height: u32) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::YUV420P10LE, width, height);
    for plane in 0..3 {
        let (data, stride) = ffmpeg_safe::plane_mut(&mut frame, plane).unwrap();
        for (row, samples) in data.chunks_mut(stride).enumerate() {
            for (x, sample) in samples.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let value = (plane * 389 + row * 61 + x * 7) % 1024;
                *sample = (value as u16).to_le_bytes();
            }
        }
    }
    tag_pq(&mut frame);
    frame
}

/// The first `bytes` of each of a plane's `rows`.
fn rows((plane, stride): (&[u8], usize), rows: usize, bytes: usize) -> Vec<&[u8]> {
    plane
        .chunks(stride)
        .take(rows)
        .map(|row| &row[..bytes])
        .collect()
}

/// The P010 rows of a 10-bit 4:2:0 frame, as P010 has them: each sample
/// in the high 10 bits, the chroma planes interleaved.
fn p010_of(frame: &frame::Video) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let sample = |plane: usize, x: usize, y: usize| {
        let (data, stride) = ffmpeg_safe::plane(frame, plane).unwrap();
        let at = y * stride + 2 * x;
        (u16::from_le_bytes([data[at], data[at + 1]]) << 6).to_le_bytes()
    };
    let luma = (0..h)
        .map(|y| (0..w).flat_map(|x| sample(0, x, y)).collect())
        .collect();
    let chroma = (0..h.div_ceil(2))
        .map(|y| {
            (0..w.div_ceil(2))
                .flat_map(|x| [sample(1, x, y), sample(2, x, y)].concat())
                .collect()
        })
        .collect();
    (luma, chroma)
}

#[test]
fn a_10_bit_frame_goes_into_p010_sample_for_sample() {
    for (width, height) in [(16, 8), (7, 5), (1, 1)] {
        let frame = planar_frame(width, height);
        let (w, h) = (width as usize, height as usize);
        let (luma_stride, chroma_stride) = (2 * w + 6, 4 * w.div_ceil(2) + 10);
        let mut luma = vec![0xab; luma_stride * h];
        let mut chroma = vec![0xab; chroma_stride * h.div_ceil(2)];
        let copied = to_p010(
            &frame,
            (&mut luma, luma_stride),
            (&mut chroma, chroma_stride),
        );
        assert!(copied.is_some(), "{width} x {height}");
        let pairs = 4 * w.div_ceil(2);
        let (expected_luma, expected_chroma) = p010_of(&frame);
        assert_eq!(
            rows((&luma, luma_stride), h, 2 * w),
            expected_luma,
            "{width} x {height}"
        );
        assert_eq!(
            rows((&chroma, chroma_stride), h.div_ceil(2), pairs),
            expected_chroma,
            "{width} x {height}"
        );
        // Past each row, left as it was.
        assert!(
            luma.chunks(luma_stride)
                .all(|row| row[2 * w..].iter().all(|&b| b == 0xab))
        );
        assert!(
            chroma
                .chunks(chroma_stride)
                .all(|row| row[pairs..].iter().all(|&b| b == 0xab))
        );
    }
    // As swscale makes them, where its chroma is whole.
    let frame = planar_frame(16, 8);
    let mut swscaled = frame::Video::new(Pixel::P010LE, 16, 8);
    scaling::Context::get(
        Pixel::YUV420P10LE,
        16,
        8,
        Pixel::P010LE,
        16,
        8,
        scaling::Flags::BILINEAR,
    )
    .unwrap()
    .run(&frame, &mut swscaled)
    .unwrap();
    let (luma, chroma) = p010_of(&frame);
    assert_eq!(rows(ffmpeg_safe::plane(&swscaled, 0).unwrap(), 8, 32), luma);
    assert_eq!(
        rows(ffmpeg_safe::plane(&swscaled, 1).unwrap(), 4, 32),
        chroma
    );
}

#[test]
fn an_uploaded_10_bit_frame_shows_its_own_samples() {
    let mut converter = Converter::new().unwrap();
    let frame = planar_frame(16, 8);
    let read = converter.upload(&frame, 16, 8).unwrap().read().unwrap();
    assert_eq!(read.format(), Pixel::P010LE);
    let (luma, chroma) = p010_of(&frame);
    assert_eq!(rows(ffmpeg_safe::plane(&read, 0).unwrap(), 8, 32), luma);
    assert_eq!(rows(ffmpeg_safe::plane(&read, 1).unwrap(), 4, 32), chroma);
    // Smaller, it's scaled.
    let surface = converter.upload(&frame, 8, 4).unwrap();
    assert_eq!(surface.size(), (8, 4));
}

#[test]
fn an_uploaded_frame_stored_bottom_up_shows_the_right_way_up() {
    let mut converter = Converter::new().unwrap();
    let (luma, chroma) = p010_of(&planar_frame(16, 8));
    // After one stored top down, as it is.
    converter.upload(&planar_frame(16, 8), 16, 8).unwrap();
    // Every plane, then luma alone, as rawvideo's BottomUp stores it.
    for planes in [3, 1] {
        let stored = stored_bottom_up(planar_frame(16, 8), planes);
        let (mut y, mut uv) = (vec![0; 32 * 8], vec![0; 32 * 4]);
        assert!(to_p010(&stored, (&mut y, 32), (&mut uv, 32)).is_none());
        let read = converter.upload(&stored, 16, 8).unwrap().read().unwrap();
        assert_eq!(read.format(), Pixel::P010LE);
        let (read_luma, read_chroma) = (
            rows(ffmpeg_safe::plane(&read, 0).unwrap(), 8, 32),
            rows(ffmpeg_safe::plane(&read, 1).unwrap(), 4, 32),
        );
        assert_eq!(read_luma, luma, "{planes} planes");
        assert_eq!(read_chroma, chroma, "{planes} planes");
    }
}

#[test]
fn an_hdr_buffer_as_drawn_is_its_own_surface() {
    let video_range = kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange;
    let pooled = || {
        pixel_buffer_pool(16, 16, video_range)
            .unwrap()
            .create_pixel_buffer()
            .unwrap()
    };
    let frame = pq_frame();
    let source = pooled();
    tag_colors(source.as_concrete_TypeRef(), &frame);
    assert!(as_drawn(&source, video_range, 16, 16));
    assert!(!as_drawn(&source, video_range, 16, 8), "another size");
    assert!(!as_drawn(&source, video_range, 8, 16), "another size");
    let full_range = kCVPixelFormatType_420YpCbCr10BiPlanarFullRange;
    assert!(!as_drawn(&source, full_range, 16, 16), "another format");
    // BT.709's matrix, or none.
    let mut bt709 = pq_frame();
    bt709.set_color_space(color::Space::BT709);
    let other = pooled();
    tag_colors(other.as_concrete_TypeRef(), &bt709);
    assert!(!as_drawn(&other, video_range, 16, 16));
    assert!(!as_drawn(&pooled(), video_range, 16, 16));
    // Not on an IOSurface, gpui can't draw it.
    let plain = CVPixelBuffer::new(video_range, 16, 16, None).unwrap();
    tag_colors(plain.as_concrete_TypeRef(), &frame);
    assert!(!as_drawn(&plain, video_range, 16, 16));

    let mut converter = Converter::new().unwrap();
    let raw = source.as_concrete_TypeRef();
    let surface = converter.convert_buffer(&frame, raw, 16, 16).unwrap();
    assert_eq!(surface.buffer().as_concrete_TypeRef(), raw);
    let smaller = converter.convert_buffer(&frame, raw, 8, 8).unwrap();
    assert_ne!(smaller.buffer().as_concrete_TypeRef(), raw);
    assert_eq!(smaller.size(), (8, 8));
    // A copy carries its source's colors.
    let transfer = CFString::from(CVImageBufferKeys::TransferFunction);
    let pq = CFString::from(CVImageBufferTransferFunction::SMPTE_ST_2084_PQ);
    let carried = smaller
        .buffer()
        .as_buffer()
        .get_attachment(&transfer, None)
        .and_then(|value| value.downcast::<CFString>());
    assert_eq!(carried, Some(pq));
    let plain = converter
        .convert_buffer(&frame, plain.as_concrete_TypeRef(), 16, 16)
        .unwrap();
    assert_ne!(plain.buffer().as_concrete_TypeRef(), raw);

    // SDR goes into BT.601 whatever its matrix, so it's always copied.
    let eight_bit = kCVPixelFormatType_420YpCbCr8BiPlanarFullRange;
    let mut sdr = frame::Video::new(Pixel::NV12, 16, 16);
    sdr.set_color_range(color::Range::JPEG);
    sdr.set_color_space(color::Space::BT2020NCL);
    let source = pixel_buffer_pool(16, 16, eight_bit)
        .unwrap()
        .create_pixel_buffer()
        .unwrap();
    tag_colors(source.as_concrete_TypeRef(), &sdr);
    assert!(as_drawn(&source, eight_bit, 16, 16));
    let raw = source.as_concrete_TypeRef();
    let copied = converter.convert_buffer(&sdr, raw, 16, 16).unwrap();
    assert_ne!(copied.buffer().as_concrete_TypeRef(), raw);
}
