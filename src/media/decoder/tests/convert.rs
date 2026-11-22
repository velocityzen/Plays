use super::*;

/// A `width` x `height` frame of 4:2:0 Y'CbCr, each sample telling
/// where it is.
fn pattern(width: u32, height: u32) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::YUV420P, width, height);
    for plane in 0..3 {
        let stride = frame.stride(plane);
        for (y, row) in frame.data_mut(plane).chunks_mut(stride).enumerate() {
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
        let expected: Vec<u8> = own
            .data(0)
            .chunks(own.stride(0))
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
