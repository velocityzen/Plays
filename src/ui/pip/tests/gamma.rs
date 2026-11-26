use super::*;
use crate::media::picture::gamma_table;

const NV12: Ycc = Ycc {
    kr: 0.2126,
    kb: 0.0722,
    bits: 8,
    full_range: false,
    shift: 0,
};

const X420: Ycc = Ycc {
    kr: 0.2627,
    kb: 0.0593,
    bits: 10,
    full_range: false,
    shift: 6,
};

fn near(a: f32, b: f32, within: f32) -> bool {
    (a - b).abs() <= within
}

#[test]
fn a_gamma_of_one_leaves_signals_as_they_are() {
    for transfer in [None, Some(Transfer::Pq), Some(Transfer::Hlg)] {
        let curve = Curve::new(1.0, transfer);
        for step in 0..=100 {
            let signal = step as f32 / 100.0;
            assert!(
                near(curve.at(signal), signal, 1e-3),
                "{transfer:?} {signal}: {}",
                curve.at(signal)
            );
        }
    }
}

#[test]
fn sdr_signals_bend_as_the_block_bends_them() {
    let curve = Curve::new(2.0, None);
    assert_eq!((curve.at(0.0), curve.at(1.0)), (0.0, 1.0));
    assert!(near(curve.at(0.25), 0.5, 1e-4));
    assert!(near(curve.at(0.3), apply_gamma(0.3, 2.0), 1e-3));
    let between = 0.5 + 0.5 / STEPS as f32;
    assert!(
        near(curve.at(between), apply_gamma(between, 2.0), 1e-5),
        "between steps"
    );
    assert!(near(Curve::new(0.5, None).at(0.5), 0.25, 1e-3), "and down");
    assert_eq!((curve.at(-1.0), curve.at(2.0)), (0.0, 1.0), "past the ends");
}

#[test]
fn hdr_keeps_black_and_sdr_white_and_bends_the_light_between_and_past() {
    for transfer in [Transfer::Pq, Transfer::Hlg] {
        let curve = Curve::new(2.0, Some(transfer));
        let signal = |light: f32| hdr::encode([light; 3], transfer)[0];
        let white = signal(1.0);
        assert!(near(curve.at(0.0), 0.0, 1e-3), "{transfer:?} black");
        assert!(near(curve.at(white), white, 2e-3), "{transfer:?} white");
        // A dark gray bent in sRGB's encoding of its light, not the light.
        let dark = signal(0.02);
        let encoded = apply_gamma(hdr::srgb_encode(0.02), 2.0);
        let expected = signal(hdr::srgb_decode(encoded));
        assert!(near(curve.at(dark), expected, 2e-3), "{transfer:?} dark");
        assert!(curve.at(dark) > dark, "{transfer:?} shadows lift");
        let highlight = signal(3.0);
        assert!(
            curve.at(highlight) < highlight,
            "{transfer:?} highlights sink"
        );
    }
}

#[test]
fn bgra_pixels_bend_by_channel_and_keep_their_alpha() {
    let curve = Curve::new(1.6, None);
    let table = gamma_table(1.6);
    // Two pixels a row, and a pixel's bytes over in each.
    let mut pixels: Vec<u8> = (0..24u8).map(|at| at.wrapping_mul(47)).collect();
    let before = pixels.clone();
    curve.bend_bgra(&mut pixels, 12, 2);
    for (at, (&now, &was)) in pixels.iter().zip(&before).enumerate() {
        let expected = match at % 12 {
            3 | 7 | 8.. => was,
            _ => table[usize::from(was)],
        };
        assert!(
            now.abs_diff(expected) <= 1,
            "byte {at}: {now} for {expected}"
        );
    }
}

/// `rgb` as one 2 × 2 block's samples, bent, and read back as R'G'B'.
fn bent_block<S: Sample + Send>(curve: &Curve, rgb: [f32; 3], ycc: Ycc) -> [f32; 3] {
    let (luma_range, chroma_range) = (ycc.luma_range(), ycc.chroma_range());
    let y: S = ycc.write(ycc.luma(rgb), luma_range);
    let (cb, cr) = ycc.chroma(rgb);
    let mut luma = vec![y; 4];
    let mut chroma: Vec<S> = vec![ycc.write(cb, chroma_range), ycc.write(cr, chroma_range)];
    curve.bend_ycbcr((&mut luma, 2), (&mut chroma, 2), 2, ycc);
    assert!(luma.iter().all(|sample| sample.code() == luma[0].code()));
    ycc.rgb(
        ycc.read(luma[0], luma_range),
        ycc.read(chroma[0], chroma_range),
        ycc.read(chroma[1], chroma_range),
    )
}

#[test]
fn ycbcr_bends_each_color_by_its_channels() {
    let curve = Curve::new(1.8, None);
    for rgb in [
        [0.5; 3],
        [0.2, 0.6, 0.9],
        [0.8, 0.3, 0.1],
        [0.0; 3],
        [1.0; 3],
    ] {
        let expected = rgb.map(|signal| curve.at(signal));
        let eight = bent_block::<u8>(&curve, rgb, NV12);
        let ten = bent_block::<u16>(&curve, rgb, X420);
        for (got, within) in [(eight, 0.02), (ten, 0.005)] {
            assert!(
                got.iter().zip(expected).all(|(a, b)| near(*a, b, within)),
                "{rgb:?}: {got:?} for {expected:?}"
            );
        }
    }
}

#[test]
fn ycbcr_of_odd_sizes_bends_every_pixel_and_no_more() {
    // 3 × 3, rows 4 samples apart, with a sample past each row's end.
    let curve = Curve::new(2.0, None);
    let (luma_range, chroma_range) = (NV12.luma_range(), NV12.chroma_range());
    let gray: u8 = NV12.write(0.25, luma_range);
    let neutral: u8 = NV12.write(0.0, chroma_range);
    let mut luma = vec![gray; 12];
    let mut chroma = vec![neutral; 8];
    curve.bend_ycbcr((&mut luma, 4), (&mut chroma, 4), 3, NV12);
    for (at, sample) in luma.iter().enumerate() {
        if at % 4 == 3 {
            assert_eq!(*sample, gray, "past the row at {at}");
        } else {
            assert!(near(NV12.read(*sample, luma_range), 0.5, 0.01), "{at}");
        }
    }
    assert!(chroma.iter().all(|&sample| sample == neutral), "{chroma:?}");
}
