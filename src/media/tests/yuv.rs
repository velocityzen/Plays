use super::*;

/// What the GPU does to a pixel's samples, read as it reads them.
fn rgb(matrix: &[[f32; 4]; 4], samples: [f32; 3]) -> [f32; 3] {
    let input = [samples[0], samples[1], samples[2], 1.];
    [0, 1, 2].map(|row| (0..4).map(|col| matrix[col][row] * input[col]).sum())
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-3)
}

#[test]
fn converts_limited_and_full_range_at_any_depth() {
    let bt709 = (0.2126, 0.0722);
    // 8-bit limited: black, white, and BT.709's red.
    let m = ycbcr_to_rgb(bt709, false, 8, 255.);
    assert!(close(
        rgb(&m, [16. / 255., 0.5019608, 0.5019608]),
        [0., 0., 0.]
    ));
    assert!(close(
        rgb(&m, [235. / 255., 0.5019608, 0.5019608]),
        [1., 1., 1.]
    ));
    let red = [62.5594, 102.335_85, 240.].map(|code| code / 255.);
    assert!(close(rgb(&m, red), [1., 0., 0.]));
    // 8-bit full range.
    let m = ycbcr_to_rgb(bt709, true, 8, 255.);
    assert!(close(rgb(&m, [1., 128. / 255., 128. / 255.]), [1., 1., 1.]));
    // 10-bit limited, in the low bits (yuv420p10) and the high (p010).
    let low = ycbcr_to_rgb(bt709, false, 10, 65535.);
    let high = ycbcr_to_rgb(bt709, false, 10, 65535. / 64.);
    let white = [940., 512., 512.];
    assert!(close(rgb(&low, white.map(|c| c / 65535.)), [1., 1., 1.]));
    assert!(close(
        rgb(&high, white.map(|c| c * 64. / 65535.)),
        [1., 1., 1.]
    ));
}

#[test]
fn hands_over_the_formats_the_gpu_converts() {
    let mut converter = YuvConverter::default();
    let mut nv12 = frame::Video::new(Pixel::NV12, 6, 4);
    nv12.set_color_range(color::Range::MPEG);
    let picture = converter.convert(&nv12, None).unwrap().unwrap();
    assert_eq!(picture.size(), (6, 4));
    assert_eq!(picture.frame.planes().len(), 2);
    assert_eq!(picture.frame.planes()[1].len(), 3 * 2 * 2);
    assert!(!picture.hdr);
    assert_eq!(picture.frame.color().chroma_siting, [0.5, 1.]);

    // Over the limit: scaled down, into planes of their own.
    let big = frame::Video::new(Pixel::P010LE, 64, 32);
    let picture = converter.convert(&big, Some(16)).unwrap().unwrap();
    assert_eq!(picture.size(), (32, 16));
    assert_eq!(picture.frame.layout(), YuvLayout::Planar);
    assert!(picture.frame.is_wide());
    assert_eq!(picture.frame.color().chroma_siting, [1., 1.]);

    let rgb = frame::Video::new(Pixel::RGB24, 4, 4);
    assert!(converter.convert(&rgb, None).is_none());
}

/// A 4:2:0 frame whose samples each say where they are: plane, row and
/// column.
fn numbered(width: u32, height: u32) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::YUV420P, width, height);
    for plane in 0..frame.planes() {
        let (data, stride) = ffmpeg_safe::plane_mut(&mut frame, plane).unwrap();
        for (y, row) in data.chunks_mut(stride).enumerate() {
            for (x, sample) in row.iter_mut().enumerate() {
                *sample = (plane * 100 + y * 10 + x) as u8;
            }
        }
    }
    frame
}

/// `frame`'s first `planes` planes stored bottom up, the picture the same:
/// their rows in reverse, the first last in memory, and their linesizes
/// negative.
pub(crate) fn stored_bottom_up(mut frame: frame::Video, planes: usize) -> frame::Video {
    for plane in 0..planes {
        let (data, stride) = ffmpeg_safe::plane_mut(&mut frame, plane).unwrap();
        let rows = data.len().div_ceil(stride);
        let width = data.len() - stride * (rows - 1);
        let reversed: Vec<u8> = data
            .chunks(stride)
            .rev()
            .flat_map(|row| &row[..width])
            .copied()
            .collect();
        for (row, picture) in data.chunks_mut(stride).zip(reversed.chunks(width)) {
            row[..width].copy_from_slice(picture);
        }
        // SAFETY: the plane holds `rows` rows of `stride` bytes; from the
        // last one's start, a negative linesize steps back through them.
        unsafe {
            let raw = frame.as_mut_ptr();
            (*raw).data[plane] = (*raw).data[plane].add(stride * (rows - 1));
            (*raw).linesize[plane] = -(*raw).linesize[plane];
        }
    }
    frame
}

#[test]
fn hands_over_a_frame_stored_bottom_up_the_right_way_up() {
    let mut converter = YuvConverter::default();
    let upright = converter.convert(&numbered(8, 6), None).unwrap().unwrap();
    let siting = |picture: &YuvPicture| picture.frame.color().chroma_siting;

    // Every plane, as Dxtory's flipped frames are.
    let stored = stored_bottom_up(numbered(8, 6), 3);
    assert!(ffmpeg_safe::plane(&stored, 2).is_none(), "stored bottom up");
    let picture = converter.convert(&stored, None).unwrap().unwrap();
    assert_eq!(picture.size(), (8, 6));
    assert_eq!(picture.frame.planes(), upright.frame.planes());
    assert_eq!(siting(&picture), siting(&upright), "chroma moved");

    // Luma only, as rawvideo's BottomUp is.
    let stored = stored_bottom_up(numbered(8, 6), 1);
    let picture = converter.convert(&stored, None).unwrap().unwrap();
    assert_eq!(picture.frame.planes(), upright.frame.planes());
}

#[test]
fn a_pictures_planes_read_back_are_its_frame_tagged_as_it_was() {
    let mut converter = YuvConverter::default();
    let mut tagged = numbered(8, 6);
    tagged.set_color_space(color::Space::BT709);
    tagged.set_color_primaries(color::Primaries::BT709);
    let read = converter
        .convert(&tagged, None)
        .unwrap()
        .unwrap()
        .read()
        .unwrap();
    assert_eq!(read.format(), Pixel::YUV420P);
    assert_eq!((read.width(), read.height()), (8, 6));
    for plane in 0..3 {
        let (width, rows) = (
            read.plane_width(plane) as usize,
            read.plane_height(plane) as usize,
        );
        let row = |frame: &frame::Video, y: usize| {
            let (data, stride) = ffmpeg_safe::plane(frame, plane).unwrap();
            data[y * stride..][..width].to_vec()
        };
        for y in 0..rows {
            assert_eq!(row(&read, y), row(&tagged, y), "plane {plane}, row {y}");
        }
    }
    let colors = Colors::of_frame(&read);
    assert_eq!(
        colors,
        Colors {
            range: color::Range::MPEG,
            ..Colors::of_frame(&tagged)
        }
    );

    // Untagged, the matrix and range it was taken to have, whatever size
    // it's at now.
    let mut read = |frame: &frame::Video, max_short_side| {
        let picture = converter.convert(frame, max_short_side).unwrap().unwrap();
        Colors::of_frame(&picture.read().unwrap())
    };
    let hd = read(&numbered(1280, 720), Some(360));
    assert_eq!(
        (hd.space, hd.range),
        (color::Space::BT709, color::Range::MPEG)
    );
    let mut jpeg = numbered(8, 6);
    // SAFETY: a field of a valid frame, set to a format of the same planes.
    unsafe { (*jpeg.as_mut_ptr()).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_YUVJ420P as i32 };
    let sd = read(&jpeg, None);
    assert_eq!(
        (sd.space, sd.range),
        (color::Space::SMPTE170M, color::Range::JPEG)
    );
}

#[test]
fn an_hdr_pictures_peak_goes_with_it_and_comes_back_read() {
    let mut converter = YuvConverter::default();
    let mut frame = frame::Video::new(Pixel::P010LE, 8, 4);
    frame.set_color_transfer_characteristic(color::TransferCharacteristic::SMPTE2084);
    hdr::attach_peak(&mut frame, 1000.0).unwrap();
    // As it is, and scaled down.
    for max_short_side in [None, Some(2)] {
        let picture = converter.convert(&frame, max_short_side).unwrap().unwrap();
        assert_eq!(picture.frame.color().peak_nits, Some(1000.0));
        let read = picture.read().unwrap();
        assert_eq!(hdr::peak_nits(&read), Some(1000.0), "{max_short_side:?}");
    }
    let sdr = frame::Video::new(Pixel::P010LE, 8, 4);
    let picture = converter.convert(&sdr, None).unwrap().unwrap();
    assert_eq!(picture.frame.color().peak_nits, None);
}
