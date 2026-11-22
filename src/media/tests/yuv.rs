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
