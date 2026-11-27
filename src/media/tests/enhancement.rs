use super::*;
use crate::media::dovi::tests::attach_rpu;
use crate::media::hdr::tests::attach_hdr10;
use crate::media::yuv::tests::stored_bottom_up;

/// A profile 7 RPU, as ffprobe prints a Blu-ray's but plainer: 10-bit
/// layers, each component mapped as it is, the enhancement layer
/// dequantized around 512 at 1/512 a code, mastered at 4,000 nits.
fn profile7() -> (
    sys::RpuDataHeader,
    Box<sys::DataMapping>,
    sys::ColorMetadata,
) {
    // SAFETY: all-zero is valid for these plain structs.
    let mut header: sys::RpuDataHeader = unsafe { std::mem::zeroed() };
    header.rpu_type = 2;
    header.rpu_format = 18;
    header.vdr_rpu_profile = 1;
    header.coef_log2_denom = 23;
    header.bl_bit_depth = 10;
    header.el_bit_depth = 10;
    header.vdr_bit_depth = 12;
    header.el_spatial_resampling_filter_flag = 1;
    // SAFETY: as for the header.
    let mut mapping: Box<sys::DataMapping> = Box::new(unsafe { std::mem::zeroed() });
    for curve in &mut mapping.curves {
        as_it_is(curve);
    }
    mapping.nlq_method_idc = sys::NLQ_LINEAR_DZ;
    mapping.num_x_partitions = 1;
    mapping.num_y_partitions = 1;
    mapping.nlq = [sys::NlqParams {
        nlq_offset: 512,
        vdr_in_max: 1 << 20,
        linear_deadzone_slope: 1 << 14,
        linear_deadzone_threshold: 0,
    }; 3];
    // SAFETY: as for the header.
    let mut color: sys::ColorMetadata = unsafe { std::mem::zeroed() };
    color.source_max_pq = 3696;
    (header, mapping, color)
}

/// One piece between the first code and the last, mapping each to itself.
fn as_it_is(curve: &mut sys::ReshapingCurve) {
    curve.num_pivots = 2;
    curve.pivots[..2].copy_from_slice(&[0, 1023]);
    curve.mapping_idc[0] = 0;
    curve.poly_order[0] = 1;
    curve.poly_coef[0] = [0, 1 << 23, 0];
}

fn composer() -> Composer {
    let (header, mapping, color) = profile7();
    Composer::new(&header, &mapping, &color).unwrap()
}

/// A 10-bit 4:2:0 frame, each component's codes from `code(component, x,
/// y)`.
fn frame(
    format: Pixel,
    (width, height): (u32, u32),
    code: impl Fn(usize, usize, usize) -> u16,
) -> frame::Video {
    let mut frame = frame::Video::new(format, width, height);
    let (chroma_width, chroma_height) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
    let mut put = |plane: usize, x: usize, y: usize, value: u16| {
        let (data, stride) = ffmpeg_safe::plane_mut(&mut frame, plane).unwrap();
        data[y * stride + 2 * x..][..2].copy_from_slice(&value.to_le_bytes());
    };
    let shift = if format == Pixel::P010LE { 6 } else { 0 };
    for y in 0..height as usize {
        for x in 0..width as usize {
            put(0, x, y, code(0, x, y) << shift);
        }
    }
    for y in 0..chroma_height {
        for x in 0..chroma_width {
            for c in 1..3 {
                match format {
                    Pixel::P010LE => put(1, 2 * x + c - 1, y, code(c, x, y) << shift),
                    _ => put(c, x, y, code(c, x, y)),
                }
            }
        }
    }
    frame
}

/// A frame's codes, component by component, row by row.
fn codes(frame: &frame::Video) -> Vec<Vec<Vec<u16>>> {
    let planes = Planes::of(frame).unwrap();
    planes
        .0
        .iter()
        .map(|plane| (0..plane.height).map(|y| plane.row(y)).collect())
        .collect()
}

/// `frame`'s codes, each `by` more.
fn raised(frame: &frame::Video, by: u16) -> Vec<Vec<Vec<u16>>> {
    let mut codes = codes(frame);
    codes.iter_mut().flatten().flatten().for_each(|c| *c += by);
    codes
}

/// A change to an RPU.
type Change = fn(&mut sys::RpuDataHeader, &mut sys::DataMapping);

#[test]
fn composes_only_what_it_can() {
    let ok = |change: Change| {
        let (mut header, mut mapping, color) = profile7();
        change(&mut header, &mut mapping);
        Composer::new(&header, &mapping, &color).is_some()
    };
    assert!(ok(|_, _| {}));
    assert!(ok(|h, _| h.coef_log2_denom = 32));
    let cases: [(&str, Change); 12] = [
        ("no residual, as profile 8", |h, _| {
            h.disable_residual_flag = 1
        }),
        ("IPT, as profile 5", |_, m| m.mapping_color_space = 1),
        ("no dequantizing", |_, m| m.nlq_method_idc = -1),
        ("partitions across", |_, m| m.num_x_partitions = 2),
        ("partitions down", |_, m| m.num_y_partitions = 2),
        ("an 8-bit base layer", |h, _| h.bl_bit_depth = 8),
        ("a 12-bit base layer", |h, _| h.bl_bit_depth = 12),
        ("an 8-bit enhancement layer", |h, _| h.el_bit_depth = 8),
        ("an 11-bit enhancement layer", |h, _| h.el_bit_depth = 11),
        ("a denominator past 32 bits", |h, _| h.coef_log2_denom = 33),
        ("luma by MMR", |_, m| {
            m.curves[0].mapping_idc[0] = sys::MAPPING_MMR
        }),
        ("chroma of one pivot", |_, m| m.curves[1].num_pivots = 1),
    ];
    for (what, change) in cases {
        assert!(!ok(change), "{what}");
    }
    assert!(!ok(|_, m| m.curves[2].num_pivots = 10), "ten pivots");
}

#[test]
fn polynomials_map_as_the_spec_says() {
    // As it is: a code over 1,024, in 16 bits.
    for s in [0, 1, 512, 1023] {
        assert_eq!(polynomial(s, 1, &[0, 1 << 23, 0], 10, 23), (s * 64) as u16);
    }
    // 0.25 + 0.5x², at x = 0.5; and without x², of order 1.
    let quadratic = [1 << 21, 0, 1 << 22];
    assert_eq!(polynomial(512, 2, &quadratic, 10, 23), 24576);
    assert_eq!(polynomial(512, 1, &quadratic, 10, 23), 16384);
    // Under 0 is 0, and past 1 the most.
    assert_eq!(polynomial(512, 1, &[-(1 << 23), 0, 0], 10, 23), 0);
    assert_eq!(polynomial(512, 1, &[-(1 << 21), 0, 0], 10, 23), 0);
    assert_eq!(polynomial(512, 1, &[2 << 23, 0, 0], 10, 23), 0xffff);
    // Over 256 for 8 bits, and fewer bits of fraction.
    assert_eq!(polynomial(128, 1, &[0, 1 << 23, 0], 8, 23), 32768);
    assert_eq!(polynomial(512, 1, &[0, 1 << 10, 0], 10, 10), 32768);
}

#[test]
fn luma_maps_by_a_polynomial_up_to_the_second_order() {
    for (order, expected) in [(1, 16384), (2, 24576), (3, 24576)] {
        let (header, mut mapping, color) = profile7();
        // 0.25 + 0.5x², at x = 0.5.
        let luma = &mut mapping.curves[0];
        luma.poly_order[0] = order;
        luma.poly_coef[0] = [1 << 21, 0, 1 << 22];
        let composer = Composer::new(&header, &mapping, &color).unwrap();
        assert_eq!(
            composer.mapped[0].as_ref().unwrap()[512],
            expected,
            "order {order}"
        );
    }
}

#[test]
fn the_pivots_pick_the_piece_and_bound_the_code() {
    let (header, mut mapping, color) = profile7();
    let luma = &mut mapping.curves[0];
    luma.num_pivots = 3;
    luma.pivots[..3].copy_from_slice(&[100, 512, 900]);
    // As it is, then a half.
    luma.poly_order[..2].copy_from_slice(&[1, 1]);
    luma.poly_coef[1] = [1 << 22, 0, 0];
    let composer = Composer::new(&header, &mapping, &color).unwrap();
    let mapped = composer.mapped[0].as_ref().unwrap();
    assert_eq!(mapped.len(), 1024);
    assert_eq!(mapped[511], 511 * 64, "the first piece");
    assert_eq!(mapped[512], 32768, "the second");
    assert_eq!(mapped[900], 32768, "the last pivot, of the last piece");
    assert_eq!(mapped[1023], 32768);
    assert_eq!(mapped[50], 100 * 64, "below the first pivot, as at it");
}

#[test]
fn mmr_maps_by_all_three_components() {
    let mmr = |constant, coefficients| Mmr {
        constant,
        coefficients,
    };
    let none = [[0.0; 7]; 3];
    assert_eq!(mmr(0.5, none).map([0.1, 0.2, 0.3]), 32768);
    let samples = [0.5, 0.25, 0.75];
    // s₀, s₁, s₂, s₀s₁, s₀s₂, s₁s₂, s₀s₁s₂.
    let terms: [f32; 7] = [0.5, 0.25, 0.75, 0.125, 0.375, 0.1875, 0.093_75];
    for (k, term) in terms.into_iter().enumerate() {
        for power in 0..3 {
            let mut coefficients = none;
            coefficients[power][k] = 1.0;
            let expected = (term.powi(power as i32 + 1) * 65536.0) as u16;
            assert_eq!(
                mmr(0.0, coefficients).map(samples),
                expected,
                "term {k} to the power of {}",
                power + 1
            );
        }
    }
    assert_eq!(mmr(-1.0, none).map(samples), 0);
    assert_eq!(mmr(2.0, none).map(samples), 0xffff);
}

#[test]
fn mmr_takes_coefficients_up_to_its_order() {
    for order in 1..=3 {
        let (header, mut mapping, color) = profile7();
        let cb = &mut mapping.curves[1];
        cb.mapping_idc[0] = sys::MAPPING_MMR;
        cb.mmr_order[0] = order as u8;
        cb.mmr_constant[0] = 1 << 22;
        cb.mmr_coef[0] = [[1 << 23; 7]; 3];
        let composer = Composer::new(&header, &mapping, &color).unwrap();
        assert!(composer.mapped[1].is_none(), "MMR has no table");
        let Piece::Mmr(mmr) = composer.curves[1].pieces[0] else {
            panic!("Cb is MMR");
        };
        assert_eq!(mmr.constant, 0.5);
        for (power, weights) in mmr.coefficients.iter().enumerate() {
            let weight = if power < order { 1.0 } else { 0.0 };
            assert_eq!(*weights, [weight; 7], "order {order}, power {}", power + 1);
        }
    }
    // In fewer bits of fraction, the same numbers are worth more.
    let (mut header, mut mapping, color) = profile7();
    header.coef_log2_denom = 22;
    let cb = &mut mapping.curves[1];
    cb.mapping_idc[0] = sys::MAPPING_MMR;
    cb.mmr_order[0] = 1;
    cb.mmr_constant[0] = 1 << 20;
    cb.mmr_coef[0][0][0] = 1 << 22;
    let composer = Composer::new(&header, &mapping, &color).unwrap();
    let Piece::Mmr(mmr) = composer.curves[1].pieces[0] else {
        panic!("Cb is MMR");
    };
    assert_eq!((mmr.constant, mmr.coefficients[0][0]), (0.25, 1.0));
}

#[test]
fn the_residual_is_linear_with_a_dead_zone() {
    // 1/4,096 a code, a step of 1/1,024, at most 1/8.
    let nlq = Nlq {
        offset: 512,
        in_max: 1 << 20,
        slope: 1 << 11,
        threshold: 1 << 13,
    };
    assert_eq!(dequantize(512, nlq, 10, 23), 0, "at the offset");
    // In 16 bits: the step and half a code's slope, 64 + 8, then 16 a code.
    assert_eq!(dequantize(513, nlq, 10, 23), 72);
    assert_eq!(dequantize(514, nlq, 10, 23), 88);
    assert_eq!(dequantize(511, nlq, 10, 23), -72);
    assert_eq!(dequantize(510, nlq, 10, 23), -88);
    assert_eq!(
        dequantize(1023, nlq, 10, 23),
        8192,
        "as much as there can be"
    );
    assert_eq!(dequantize(0, nlq, 10, 23), -8192);
    // An 8-bit layer's code is worth as much.
    let eight = Nlq { offset: 128, ..nlq };
    assert_eq!(dequantize(129, eight, 8, 23), 72);
    assert_eq!(dequantize(127, eight, 8, 23), -72);
    // A denominator too small to shift down by: 1/16, half a code.
    let coarse = Nlq {
        offset: 512,
        in_max: 16,
        slope: 1,
        threshold: 0,
    };
    assert_eq!(dequantize(513, coarse, 10, 4), 2048);
}

#[test]
fn the_master_is_rounded_to_ten_bits() {
    assert_eq!(reconstruct(31, 0), 0);
    assert_eq!(reconstruct(32, 0), 1);
    assert_eq!(reconstruct(400 * 64, 0), 400);
    assert_eq!(reconstruct(400 * 64, 64), 401);
    assert_eq!(reconstruct(400 * 64, -32), 400);
    assert_eq!(reconstruct(400 * 64, -33), 399);
    assert_eq!(reconstruct(0, -8192), 0);
    assert_eq!(reconstruct(0xffff, 8192), 1023);
}

#[test]
fn chroma_takes_the_luma_around_it() {
    let top = [10, 20, 30, 40, 50, 60];
    let bottom = [11, 21, 31, 41, 51, 63];
    // Columns 1, 2 and 3 of both: (120 + 124 + 4) / 8.
    assert_eq!(downsampled_luma(&top, &bottom, 1), 31);
    // The first: column 0 stands in for the one before it.
    assert_eq!(downsampled_luma(&top, &bottom, 0), 13);
    // (200 + 206 + 4) / 8; and past the end, the last column twice.
    assert_eq!(downsampled_luma(&top, &bottom, 2), 51);
    assert_eq!(downsampled_luma(&top[..5], &bottom[..5], 2), 48);
}

fn plane(rows: &[&[u16]]) -> Codes {
    Codes {
        codes: rows.concat(),
        width: rows[0].len(),
        height: rows.len(),
    }
}

#[test]
fn the_enhancement_layer_doubles_by_annex_bs_filters() {
    let flat = plane(&[&[700u16; 6] as &[u16]; 4]);
    for chroma in [false, true] {
        for y in 0..8 {
            assert_eq!(enhancement_row(&flat, chroma, y, 12, true), [700; 12]);
        }
    }
    // As big as the base layer, it is as it is.
    let rows = plane(&[&[0, 1, 2, 3], &[4, 5, 6, 7], &[8, 9, 10, 11]]);
    assert_eq!(enhancement_row(&rows, false, 1, 4, false), [4, 5, 6, 7]);
    assert_eq!(enhancement_row(&rows, false, 9, 4, false), [8, 9, 10, 11]);
    // Down: luma by four rows, chroma by two.
    let down = plane(&[&[100; 2], &[200; 2], &[300; 2], &[400; 2]]);
    let at = |chroma, y| enhancement_row(&down, chroma, y, 4, true)[0];
    // (-3·100 + 29·100 + 111·200 - 9·300 + 64) / 128, the row above the
    // first being the first.
    assert_eq!(at(false, 2), 173);
    // (-9·100 + 111·200 + 29·300 - 3·400 + 64) / 128.
    assert_eq!(at(false, 3), 225);
    // (-3·100 + 29·200 + 111·300 - 9·400 + 64) / 128: two rows up is the
    // first.
    assert_eq!(at(false, 4), 275);
    // The rows past the last are the last.
    assert_eq!(at(false, 7), 407);
    // (64·100 + 192·200 + 128) / 256, and (192·200 + 64·300 + 128) / 256.
    assert_eq!(at(true, 2), 175);
    assert_eq!(at(true, 3), 225);
    // Across: each sample, then eight taps for the one after it.
    let step = plane(&[&[0, 0, 0, 1000, 1000, 1000, 1000, 1000]]);
    let row = enhancement_row(&step, false, 0, 16, true);
    for m in 0..8 {
        assert_eq!(row[2 * m], step.codes[m], "sample {m}");
    }
    // (2456 - 524 + 94 + 22) · 1000 / 4096, rounded.
    assert_eq!(row[5], 500);
    // Ringing below nothing is nothing, and over the top, kept.
    assert_eq!(row[3], 0);
    assert_eq!(row[7], 1100);
    // A row cut short of the last sample.
    assert_eq!(enhancement_row(&step, false, 0, 15, true).len(), 15);
}

#[test]
fn reads_p010_and_planar_alike() {
    let code = |c: usize, x: usize, y: usize| (100 * c + 10 * y + x) as u16;
    for format in [Pixel::YUV420P10LE, Pixel::P010LE] {
        let frame = frame(format, (4, 2), code);
        let planes = Planes::of(&frame).unwrap();
        assert_eq!(planes.0[0].row(1), [10, 11, 12, 13], "{format:?}");
        assert_eq!(planes.0[1].row(0), [100, 101], "{format:?}");
        assert_eq!(planes.0[2].row(0), [200, 201], "{format:?}");
    }
    let odd = frame(Pixel::YUV420P10LE, (5, 3), code);
    let planes = Planes::of(&odd).unwrap();
    assert_eq!((planes.0[1].width, planes.0[1].height), (3, 2));
    assert!(Planes::of(&frame::Video::new(Pixel::YUV420P, 4, 2)).is_none());
    assert!(Planes::of(&frame::Video::new(Pixel::YUV420P10LE, 0, 0)).is_none());
}

#[test]
fn composing_adds_the_residual_to_the_mapped_base_layer() {
    let composer = composer();
    let base = frame(Pixel::YUV420P10LE, (8, 4), |c, x, _| {
        [400, 500, 600][c] + x as u16
    });
    let flat = |size, code| frame(Pixel::YUV420P10LE, size, move |_, _, _| code);
    let mut out = frame::Video::empty();
    composer
        .compose(&base, &flat((8, 4), 512), &mut out)
        .unwrap();
    assert_eq!(codes(&out), codes(&base), "at the offset, nothing");
    // Ten codes over, a 16-bit 1,216 (9.5/512): 19 10-bit codes.
    composer
        .compose(&base, &flat((8, 4), 522), &mut out)
        .unwrap();
    assert_eq!(codes(&out), raised(&base, 19));
    // Half as big each way, doubled.
    composer
        .compose(&base, &flat((4, 2), 522), &mut out)
        .unwrap();
    assert_eq!(codes(&out), raised(&base, 19));
    // Off the GPU.
    let p010 = frame(Pixel::P010LE, (8, 4), |c, x, _| {
        [400, 500, 600][c] + x as u16
    });
    composer
        .compose(&p010, &flat((8, 4), 522), &mut out)
        .unwrap();
    assert_eq!(codes(&out), raised(&base, 19));
    for wrong in [flat((6, 4), 512), flat((8, 2), 512), flat((16, 8), 512)] {
        let size = (wrong.width(), wrong.height());
        assert!(
            composer.compose(&base, &wrong, &mut out).is_err(),
            "{size:?}"
        );
    }
    let eight = frame::Video::new(Pixel::YUV420P, 8, 4);
    assert!(
        composer
            .compose(&eight, &flat((8, 4), 512), &mut out)
            .is_err()
    );
    assert!(composer.compose(&base, &eight, &mut out).is_err());
}

#[test]
fn an_enhancement_layer_as_big_as_the_base_layer_goes_sample_for_sample() {
    let composer = composer();
    let base = frame(Pixel::YUV420P10LE, (8, 4), |_, _, _| 400);
    // Ten codes over the offset in every other column: 19 codes more.
    let layer = frame(Pixel::YUV420P10LE, (8, 4), |_, x, _| {
        if x % 2 == 0 { 522 } else { 512 }
    });
    let mut out = frame::Video::empty();
    composer.compose(&base, &layer, &mut out).unwrap();
    let codes = codes(&out);
    assert_eq!(codes[0][3], [419, 400, 419, 400, 419, 400, 419, 400]);
    assert_eq!(codes[1][1], [419, 400, 419, 400]);
}

#[test]
fn refuses_a_layer_stored_bottom_up() {
    let composer = composer();
    let flat = || frame(Pixel::YUV420P10LE, (8, 4), |_, _, _| 512);
    let mut out = frame::Video::empty();
    assert!(composer.compose(&flat(), &flat(), &mut out).is_ok());
    let flipped = stored_bottom_up(flat(), 1);
    assert!(composer.compose(&flipped, &flat(), &mut out).is_err());
    assert!(composer.compose(&flat(), &flipped, &mut out).is_err());
}

#[test]
fn composes_into_its_own_frame_again_unless_its_shared() {
    let composer = composer();
    let base = frame(Pixel::YUV420P10LE, (8, 4), |c, x, _| {
        [400, 500, 600][c] + x as u16
    });
    let flat = |code| frame(Pixel::YUV420P10LE, (8, 4), move |_, _, _| code);
    let mut out = frame::Video::empty();
    composer.compose(&base, &flat(512), &mut out).unwrap();
    let pictures = ffmpeg_safe::plane(&out, 0).unwrap().0.as_ptr();
    composer.compose(&base, &flat(512), &mut out).unwrap();
    assert_eq!(
        ffmpeg_safe::plane(&out, 0).unwrap().0.as_ptr(),
        pictures,
        "the same pictures, again"
    );
    // Another reference to them, as a frame shown keeps.
    let mut shown = frame::Video::empty();
    // SAFETY: both frames are valid, and the new one is blank.
    assert!(unsafe { ffi::av_frame_ref(shown.as_mut_ptr(), out.as_ptr()) } >= 0);
    composer.compose(&base, &flat(522), &mut out).unwrap();
    assert_eq!(codes(&shown), codes(&base), "left as it was");
    assert_eq!(codes(&out), raised(&base, 19));
}

#[test]
fn mmr_takes_the_other_chroma_and_both_rows_of_luma() {
    let (header, mut mapping, color) = profile7();
    // Cb: Cr. Cr: half of luma.
    let cb = &mut mapping.curves[1];
    cb.mapping_idc[0] = sys::MAPPING_MMR;
    cb.mmr_order[0] = 1;
    cb.mmr_coef[0][0][2] = 1 << 23;
    let cr = &mut mapping.curves[2];
    cr.mapping_idc[0] = sys::MAPPING_MMR;
    cr.mmr_order[0] = 1;
    cr.mmr_coef[0][0][0] = 1 << 22;
    let composer = Composer::new(&header, &mapping, &color).unwrap();
    // Luma 600 in the first row and 200 in the second.
    let base = frame(Pixel::YUV420P10LE, (4, 2), |c, _, y| match c {
        0 => [600, 200][y],
        1 => 100,
        _ => 700,
    });
    let flat = frame(Pixel::YUV420P10LE, (4, 2), |_, _, _| 512);
    let mut out = frame::Video::empty();
    composer.compose(&base, &flat, &mut out).unwrap();
    let codes = codes(&out);
    assert_eq!(codes[1], [[700, 700]], "Cr's");
    assert_eq!(codes[2], [[200, 200]], "half of 400, both rows' luma");
}

#[test]
fn a_curve_of_mmr_then_a_polynomial_takes_each_where_it_is() {
    let (header, mut mapping, color) = profile7();
    // Cb: a quarter below 512, as it is from there.
    let cb = &mut mapping.curves[1];
    cb.num_pivots = 3;
    cb.pivots[..3].copy_from_slice(&[0, 512, 1023]);
    cb.mapping_idc[..2].copy_from_slice(&[sys::MAPPING_MMR, 0]);
    cb.mmr_order[0] = 1;
    cb.mmr_constant[0] = 1 << 21;
    cb.poly_order[1] = 1;
    cb.poly_coef[1] = [0, 1 << 23, 0];
    let composer = Composer::new(&header, &mapping, &color).unwrap();
    let base = frame(Pixel::YUV420P10LE, (4, 2), |c, x, _| match c {
        1 => [100, 700][x],
        _ => 512,
    });
    let flat = frame(Pixel::YUV420P10LE, (4, 2), |_, _, _| 512);
    let mut out = frame::Video::empty();
    composer.compose(&base, &flat, &mut out).unwrap();
    assert_eq!(codes(&out)[1], [[256, 700]]);
}

#[test]
fn an_upscaled_code_past_the_last_is_dequantized_all_the_same() {
    let composer = composer();
    assert_eq!(composer.residuals[0].len(), 1024);
    for e in [1023u16, 1024, 1100, u16::MAX] {
        assert_eq!(
            composer.residual(0, e),
            dequantize(i64::from(e), composer.nlq[0], 10, 23),
            "{e}"
        );
    }
    assert_eq!(composer.residual(0, 1100), 8192, "as much as there can be");
}

#[test]
fn chroma_maps_by_mmr_over_the_luma_around_it() {
    let (header, mut mapping, color) = profile7();
    // Cb: half of luma. Cr: as it is below 512, a quarter above.
    let cb = &mut mapping.curves[1];
    cb.mapping_idc[0] = sys::MAPPING_MMR;
    cb.mmr_order[0] = 1;
    cb.mmr_coef[0][0][0] = 1 << 22;
    let cr = &mut mapping.curves[2];
    cr.num_pivots = 3;
    cr.pivots[..3].copy_from_slice(&[0, 512, 1023]);
    cr.mapping_idc[1] = sys::MAPPING_MMR;
    cr.mmr_order[1] = 1;
    cr.mmr_constant[1] = 1 << 21;
    let composer = Composer::new(&header, &mapping, &color).unwrap();
    let base = frame(Pixel::YUV420P10LE, (4, 2), |c, x, _| match c {
        0 => 600,
        1 => 300,
        _ => [100, 700][x],
    });
    let flat = frame(Pixel::YUV420P10LE, (4, 2), |_, _, _| 512);
    let mut out = frame::Video::empty();
    composer.compose(&base, &flat, &mut out).unwrap();
    let codes = codes(&out);
    assert_eq!(codes[1], [[300, 300]], "half of 600");
    assert_eq!(codes[2], [[100, 256]], "as it is, then a quarter");
    // Each clamped to its curve: luma past its last pivot is as at it.
    let (header, mut mapping, color) = profile7();
    mapping.curves[0].pivots[..2].copy_from_slice(&[0, 512]);
    let cb = &mut mapping.curves[1];
    cb.mapping_idc[0] = sys::MAPPING_MMR;
    cb.mmr_order[0] = 1;
    cb.mmr_coef[0][0][0] = 1 << 23;
    let composer = Composer::new(&header, &mapping, &color).unwrap();
    composer.compose(&base, &flat, &mut out).unwrap();
    assert_eq!(Planes::of(&out).unwrap().0[1].row(0), [512, 512]);
}

#[test]
fn a_composed_frame_is_hdr10_at_the_masters_peak() {
    let (header, mapping, metadata) = profile7();
    let composer = Composer::new(&header, &mapping, &metadata).unwrap();
    let mut base = frame(Pixel::YUV420P10LE, (4, 2), |_, _, _| 512);
    base.set_pts(Some(42));
    base.set_color_transfer_characteristic(color::TransferCharacteristic::BT709);
    attach_hdr10(&mut base, Some(1000), Some((1000, 1, true)));
    attach_rpu(&mut base, &header, &mapping, &metadata);
    let layer = frame(Pixel::YUV420P10LE, (4, 2), |_, _, _| 512);
    let mut out = frame::Video::empty();
    for _ in 0..2 {
        composer.compose(&base, &layer, &mut out).unwrap();
    }
    assert_eq!(out.format(), Pixel::YUV420P10LE);
    assert_eq!(out.pts(), Some(42));
    let colors = crate::media::colors::Colors::of_frame(&out);
    assert_eq!(colors.space, color::Space::BT2020NCL);
    assert_eq!(colors.primaries, color::Primaries::BT2020);
    assert_eq!(colors.transfer, color::TransferCharacteristic::SMPTE2084);
    assert_eq!(colors.range, color::Range::MPEG);
    assert!(dovi::rpu(&out).is_none(), "composed by its RPU already");
    // 3,696 as 12-bit PQ, as a whole number of nits.
    assert_eq!(hdr::peak_nits(&out), Some(4000.0));
    // SAFETY: reading the count of a valid frame's side data.
    let kinds = unsafe { (*out.as_ptr()).nb_side_data };
    assert_eq!(kinds, 1, "only the peak, once, however often composed into");

    // An RPU that doesn't say leaves the base layer's own.
    let (header, mapping, mut metadata) = profile7();
    metadata.source_max_pq = 0;
    let composer = Composer::new(&header, &mapping, &metadata).unwrap();
    composer.compose(&base, &layer, &mut out).unwrap();
    assert_eq!(hdr::peak_nits(&out), Some(1000.0));
    assert!(dovi::rpu(&out).is_none());
    // SAFETY: as above.
    let kinds = unsafe { (*out.as_ptr()).nb_side_data };
    assert_eq!(
        kinds, 2,
        "the base layer's light level and display, and no peak before"
    );
}

#[test]
fn takes_its_rpu_from_a_frames_side_data() {
    let (header, mapping, color) = profile7();
    let mut frame = frame(Pixel::YUV420P10LE, (2, 2), |_, _, _| 512);
    assert!(Composer::of(&frame).is_none());
    attach_rpu(&mut frame, &header, &mapping, &color);
    let composer = Composer::of(&frame).unwrap();
    assert_eq!(composer.peak_nits, Some(3999.685));
}

/// Codec parameters with a Dolby Vision configuration record, if given.
fn parameters(record: Option<&[u8]>) -> codec::Parameters {
    let mut parameters = codec::Parameters::new();
    if let Some(record) = record {
        // SAFETY: FFmpeg allocates the side data as big as asked, owned by
        // the parameters.
        unsafe {
            let raw = parameters.as_mut_ptr();
            let side = ffi::av_packet_side_data_new(
                &mut (*raw).coded_side_data,
                &mut (*raw).nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
                record.len(),
                0,
            );
            assert!(!side.is_null());
            std::ptr::copy_nonoverlapping(record.as_ptr(), (*side).data, record.len());
        }
    }
    parameters
}

#[test]
fn a_streams_configuration_says_which_layers_it_has() {
    // Version 1.0, profile 7, level 6, an RPU, then the enhancement layer
    // and the base layer.
    let record = |el, bl| parameters(Some(&[1, 0, 7, 6, 1, el, bl, 6, 0]));
    assert_eq!(
        Configuration::of(&record(1, 1)),
        Some(Configuration {
            profile: 7,
            level: 6,
            compatibility: 6,
            base: true,
            enhancement: true,
        })
    );
    assert!(!Configuration::is_layer_only(&record(1, 1)));
    assert!(Configuration::is_layer_only(&record(1, 0)));
    assert!(!Configuration::is_layer_only(&record(0, 1)));
    assert_eq!(Configuration::of(&parameters(None)), None);
    assert!(!Configuration::is_layer_only(&parameters(None)));
    assert_eq!(
        Configuration::of(&parameters(Some(&[1, 0, 7, 6, 1, 1, 0, 6]))),
        None,
        "cut short"
    );
}

/// Bits written most significant first, as an RPU's are.
#[derive(Default)]
pub(crate) struct Bits {
    bytes: Vec<u8>,
    /// Bits taken in the last byte, 0 when it's full or there's none.
    used: u32,
}

impl Bits {
    pub(crate) fn put(&mut self, value: u64, bits: u32) {
        for bit in (0..bits).rev() {
            if self.used == 0 {
                self.bytes.push(0);
            }
            let last = self.bytes.last_mut().unwrap();
            *last |= (((value >> bit) & 1) as u8) << (7 - self.used);
            self.used = (self.used + 1) % 8;
        }
    }

    /// Exp-Golomb, unsigned.
    pub(crate) fn ue(&mut self, value: u64) {
        let coded = value + 1;
        let bits = u64::BITS - coded.leading_zeros();
        self.put(0, bits - 1);
        self.put(coded, bits);
    }

    /// Exp-Golomb, signed.
    fn se(&mut self, value: i64) {
        let magnitude = value.unsigned_abs();
        self.ue(if value > 0 {
            2 * magnitude - 1
        } else {
            2 * magnitude
        });
    }

    /// A coefficient with `DENOM` bits of fraction: its whole part,
    /// rounded down, Exp-Golomb, then the fraction. Only a signed one is
    /// under 0.
    pub(crate) fn coefficient(&mut self, value: i64, signed: bool) {
        let whole = value >> DENOM;
        if signed {
            self.se(whole);
        } else {
            self.ue(whole.try_into().unwrap());
        }
        self.put((value & ((1 << DENOM) - 1)).cast_unsigned(), DENOM);
    }

    fn align(&mut self) {
        if self.used > 0 {
            self.put(0, 8 - self.used);
        }
    }
}

/// The bits of fraction a test RPU's coefficients have.
pub(crate) const DENOM: u32 = 23;

/// CRC-32/MPEG-2, which an RPU ends with.
fn crc32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(u32::MAX, |mut crc, &byte| {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x04c1_1db7
            };
        }
        crc
    })
}

/// A Dolby Vision profile 7 RPU as an encoder writes it, an HEVC NAL unit
/// of type 62: 10-bit layers, luma raised by a quarter, chroma as it is,
/// and from the enhancement layer about a sixteenth of each code's way from
/// 512, mastered on a display whose peak is `source_max_pq`.
pub(crate) fn rpu_nal(source_max_pq: u16) -> Vec<u8> {
    let mut bits = Bits::default();
    // The header: type 2, format 18, profile 1, level 0, then the sequence:
    // fixed point, normalized, limited range, 10-bit layers, 12-bit master.
    bits.put(2, 6);
    bits.put(18, 11);
    bits.put(1, 4);
    bits.put(0, 4);
    for (value, width) in [(1, 1), (0, 1), (0, 2)] {
        bits.put(value, width);
    }
    bits.ue(u64::from(DENOM));
    bits.put(1, 2);
    bits.put(0, 1);
    for depth in [10, 10, 12] {
        bits.ue(depth - 8);
    }
    // No filters, no compression, a residual; metadata, all of it anew.
    for (value, width) in [(0, 1), (0, 3), (0, 1), (0, 1), (1, 1), (0, 1)] {
        bits.put(value, width);
    }
    // Mapping 0, in Y'CbCr 4:2:0: each component one piece over all codes.
    for _ in 0..3 {
        bits.ue(0);
    }
    for _ in 0..3 {
        bits.ue(0);
        bits.put(0, 10);
        bits.put(1023, 10);
    }
    // Dequantized linearly with a dead zone, over all codes, in one
    // partition.
    bits.put(0, 3);
    bits.put(0, 10);
    bits.put(1023, 10);
    bits.ue(0);
    bits.ue(0);
    // First-order polynomials, not interpolated: a quarter and x for
    // luma, x for chroma.
    for constant in [1 << (DENOM - 2), 0, 0] {
        bits.ue(0);
        bits.ue(0);
        bits.put(0, 1);
        bits.coefficient(constant, true);
        bits.coefficient(1 << DENOM, true);
    }
    // Around 512, a sixteenth of a 10-bit code a code, at most an eighth
    // of the range, no dead zone.
    for _ in 0..3 {
        bits.put(512, 10);
        for coefficient in [1 << (DENOM - 3), 1 << (DENOM - 14), 0] {
            bits.coefficient(coefficient, false);
        }
    }
    // Display management: ids, no scene cut, BT.2020's matrices, PQ.
    bits.ue(0);
    bits.ue(0);
    bits.ue(0);
    for value in [
        9575u64, 0, 13802, 9575, 0xfe9c, 0xfaa8, 9575, 17610, 0, // Y'CbCr to RGB
    ] {
        bits.put(value, 16);
    }
    for _ in 0..3 {
        bits.put(0, 32);
    }
    for value in [7222u64, 8771, 390, 2654, 12430, 1300, 0, 422, 15962] {
        bits.put(value, 16);
    }
    bits.put(65535, 16);
    bits.put(0, 16);
    bits.put(0, 16);
    bits.put(0, 32);
    bits.put(12, 5);
    bits.put(0, 2);
    bits.put(0, 2);
    bits.put(0, 2);
    bits.put(62, 12);
    bits.put(u64::from(source_max_pq), 12);
    bits.put(42, 10);
    // No extension blocks.
    bits.ue(0);
    rpu_unit(bits)
}

/// An RPU, `bits` but its end, as an HEVC NAL unit of type 62.
pub(crate) fn rpu_unit(mut bits: Bits) -> Vec<u8> {
    bits.align();
    let crc = crc32(&bits.bytes);
    let mut payload = vec![0x19];
    payload.extend(bits.bytes);
    payload.extend(crc.to_be_bytes());
    payload.push(0x80);
    // A NAL unit's header, then its payload with emulation prevention.
    let mut nal = vec![62 << 1, 1];
    let mut zeros = 0;
    for byte in payload {
        if zeros == 2 && byte <= 3 {
            nal.push(3);
            zeros = 0;
        }
        nal.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    nal
}

#[test]
fn writes_bits_as_an_rpu_has_them() {
    let mut bits = Bits::default();
    bits.put(0b101, 3);
    bits.ue(0);
    bits.ue(4);
    bits.se(-2);
    bits.align();
    // 101, 1, 00101, 00101, then nothing to the byte.
    assert_eq!(bits.bytes, [0b1011_0010, 0b1001_0100]);
    let mut bits = Bits::default();
    bits.coefficient(3 << (DENOM - 1), true);
    // 1.5: a whole 1, se 010, then a half in 23 bits.
    assert_eq!(bits.bytes, [0b0101_0000, 0, 0, 0]);
    assert_eq!(bits.used, 26 % 8);
    let mut bits = Bits::default();
    bits.coefficient(-(1 << (DENOM - 1)), true);
    // -0.5: a whole -1, se 011, then a half.
    assert_eq!(bits.bytes, [0b0111_0000, 0, 0, 0]);
    assert_eq!(crc32(b"123456789"), 0x0376_e6e7, "CRC-32/MPEG-2's check");
}

#[test]
fn ffmpeg_reads_a_written_rpu_with_the_frame_it_comes_with() {
    let path = crate::media::tests::pq_media();
    let mut input = ffmpeg_safe::input_file(&path).unwrap();
    let parameters = input.stream(0).unwrap().parameters();
    let mut decoder = codec::Context::from_parameters(parameters)
        .unwrap()
        .decoder()
        .video()
        .unwrap();
    let (_, packet) = input.packets().next().unwrap();
    let mut data = packet.data().unwrap().to_vec();
    let rpu = rpu_nal(3696);
    data.extend(u32::try_from(rpu.len()).unwrap().to_be_bytes());
    data.extend(&rpu);
    let mut with_rpu = ffmpeg::Packet::copy(&data);
    with_rpu.set_pts(packet.pts());
    with_rpu.set_dts(packet.dts());
    decoder.send_packet(&with_rpu).unwrap();
    decoder.send_eof().unwrap();
    let mut frame = frame::Video::empty();
    decoder.receive_frame(&mut frame).unwrap();

    let composer = Composer::of(&frame).expect("its RPU");
    let luma = composer.mapped[0].as_ref().unwrap();
    assert_eq!((luma[0], luma[512], luma[767]), (16384, 49152, 65472));
    assert_eq!(luma[1023], 0xffff, "past 1");
    for chroma in &composer.mapped[1..] {
        let chroma = chroma.as_ref().unwrap();
        assert_eq!((chroma[0], chroma[512]), (0, 32768));
    }
    for residuals in &composer.residuals {
        // 4·(e − 512) ∓ 2, in 16 bits: about (e − 512) / 16 10-bit codes.
        assert_eq!(
            (residuals[512], residuals[612], residuals[412]),
            (0, 398, -398)
        );
    }
    assert_eq!(composer.peak_nits, Some(3999.685));
}
