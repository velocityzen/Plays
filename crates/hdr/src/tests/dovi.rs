use ffmpeg_next::util::format::Pixel;

use super::*;
use crate::testing::{NEUTRAL, attach_rpu, profile5, reshaping};

/// A frame with `profile5`'s RPU, mastered on a display whose peak is
/// `source_max_pq`.
fn rpu_frame(source_max_pq: u16) -> frame::Video {
    let (header, mapping, mut color) = profile5();
    color.source_max_pq = source_max_pq;
    let mut frame = frame::Video::new(Pixel::P010LE, 4, 2);
    attach_rpu(&mut frame, &header, &mapping, &color);
    frame
}

#[test]
fn reads_no_rpu_past_the_end_of_its_side_data() {
    let (header, mapping, color) = profile5();
    // The mapping last, where no part after it vouches for its end.
    let after = |offset: usize, len: usize| (offset + len).next_multiple_of(8);
    let header_offset = after(0, size_of::<sys::DoviMetadata>());
    let color_offset = after(header_offset, size_of::<sys::RpuDataHeader>());
    let mapping_offset = after(color_offset, size_of::<sys::ColorMetadata>());
    let whole = mapping_offset + size_of::<sys::DataMapping>();
    for len in [whole, whole - 1, mapping_offset + 8] {
        let mut frame = frame::Video::new(Pixel::P010LE, 2, 2);
        // SAFETY: FFmpeg makes side data `len` bytes long, aligned for any
        // struct; each part goes within it at a multiple of 8, the mapping
        // only as much of it as fits.
        unsafe {
            let side = ffi::av_frame_new_side_data(
                frame.as_mut_ptr(),
                ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_METADATA,
                len,
            );
            assert!(!side.is_null());
            let data = (*side).data;
            data.cast::<sys::DoviMetadata>().write(sys::DoviMetadata {
                header_offset,
                mapping_offset,
                color_offset,
            });
            std::ptr::copy_nonoverlapping(&header, data.add(header_offset).cast(), 1);
            std::ptr::copy_nonoverlapping(&color, data.add(color_offset).cast(), 1);
            std::ptr::copy_nonoverlapping(
                (&raw const *mapping).cast::<u8>(),
                data.add(mapping_offset),
                len - mapping_offset,
            );
        }
        assert_eq!(
            Reshaping::of(&frame).is_some(),
            len == whole,
            "{len} of {whole} bytes"
        );
    }
    assert!(Reshaping::of(&frame::Video::new(Pixel::P010LE, 2, 2)).is_none());
}

#[test]
fn reshapes_only_what_it_can_read() {
    let (mut header, mapping, color) = profile5();
    for depth in [7, 17] {
        header.bl_bit_depth = depth;
        assert!(
            Reshaping::new(&header, &mapping, &color).is_none(),
            "{depth}-bit"
        );
    }
    for depth in [8, 16] {
        header.bl_bit_depth = depth;
        assert!(
            Reshaping::new(&header, &mapping, &color).is_some(),
            "{depth}-bit"
        );
    }
    let (mut header, mut mapping, color) = profile5();
    header.coef_log2_denom = 33;
    assert!(Reshaping::new(&header, &mapping, &color).is_none());
    header.coef_log2_denom = 32;
    assert!(Reshaping::new(&header, &mapping, &color).is_some());
    for pivots in [1, MAX_PIECES as u8 + 2] {
        mapping.curves[2].num_pivots = pivots;
        assert!(
            Reshaping::new(&header, &mapping, &color).is_none(),
            "{pivots} pivots"
        );
    }
    mapping.curves[2].num_pivots = MAX_PIECES as u8 + 1;
    assert!(Reshaping::new(&header, &mapping, &color).is_some());
}

#[test]
fn the_mastering_displays_peak_is_the_videos() {
    let peak = reshaping().peak_nits().unwrap();
    assert!((peak - 1000.6).abs() < 0.1, "{peak}");
    let (_, _, mut color) = profile5();
    color.source_max_pq = 0;
    assert_eq!(mastered_peak(&color), None);
    // About 80 nits, under reference white.
    color.source_max_pq = 2000;
    assert_eq!(mastered_peak(&color), None);
    // As a frame's RPU says.
    let peak = Reshaping::of(&rpu_frame(3696))
        .unwrap()
        .peak_nits()
        .unwrap();
    assert!((peak - 4000.0).abs() < 5.0, "{peak}");
    assert_eq!(Reshaping::of(&rpu_frame(0)).unwrap().peak_nits(), None);
}

#[test]
fn picks_pieces_between_pivots() {
    let luma = &reshaping().curves[0];
    let at = |code: f32| luma.apply(code / 1024.0, [code / 1024.0; 3]);
    // The straight first piece, then x² / 2 + 1/4 from code 512 on.
    assert!((at(256.0) - 0.25).abs() < 1e-6);
    assert!((at(511.0) - 511.0 / 1024.0).abs() < 1e-6);
    assert!((at(512.0) - (0.5f32 * 0.5 / 2.0 + 0.25)).abs() < 1e-6);
    assert!((at(768.0) - (0.75f32 * 0.75 / 2.0 + 0.25)).abs() < 1e-6);
}

#[test]
fn evaluates_mmr() {
    let curve = Curve {
        pivots: vec![0.0, 1.0],
        pieces: vec![Piece::Mmr {
            order: 2,
            constant: 0.1,
            coefficients: [
                [0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0],
                [9.0; 7],
            ],
        }],
    };
    let [a, b, c] = [0.5f32, 0.4, 0.2];
    // 0.1 + 0.5·b + a·b·c + 2·(a·b)²; the third power is beyond the order.
    let expected = 0.1 + 0.5 * b + a * b * c + 2.0 * (a * b).powi(2);
    assert!((curve.apply(b, [a, b, c]) - expected).abs() < 1e-6);
}

#[test]
fn neutral_ipt_is_grey() {
    let reshaping = reshaping();
    let lms = reshaping.lms_signal([0.4, NEUTRAL, NEUTRAL]);
    assert!(lms.iter().all(|&s| (s - 0.4).abs() < 1e-5), "{lms:?}");
    let rgb = reshaping.lms_to_rgb(lms.map(pq_to_nits));
    let nits = pq_to_nits(0.4);
    assert!(
        rgb.iter().all(|&c| (c / nits - 1.0).abs() < 1e-3),
        "{rgb:?}"
    );
}

#[test]
fn shader_params_compute_the_same_light() {
    let reshaping = reshaping();
    let params = reshaping.shader_params();
    for samples in [
        [0.1, 0.45, 0.47],
        [0.6, 0.3, 0.7],
        [0.95, 0.5, 0.2],
        [0.0, 1.0, 0.0],
    ] {
        let cpu = reshaping.lms_to_rgb(reshaping.lms_signal(samples).map(pq_to_nits));
        let gpu = params.light(samples);
        for (a, b) in cpu.iter().zip(gpu) {
            assert!(
                (a - b).abs() <= 1e-3 * a.max(1.0),
                "{samples:?}: {cpu:?} {gpu:?}"
            );
        }
    }
    let bytes = params.as_bytes();
    let back = ShaderParams::from_bytes(bytes).unwrap();
    assert_eq!(back.as_bytes(), bytes);
    assert!(ShaderParams::from_bytes(&bytes[1..]).is_none());
}

/// Luma reshaped by as many pieces as an RPU has, ending well under the
/// top, and chroma by MMR, as most profile 5 streams' is, of the third
/// order: each pivot and each power's weights where the shader reads them,
/// and samples past the ends clamped as the CPU clamps them.
#[test]
fn shader_params_compute_the_same_light_by_every_piece_and_mmr() {
    let (header, mut mapping, color) = profile5();
    let one = 1i64 << header.coef_log2_denom;
    let fixed = |value: f64| (value * one as f64) as i64;
    let luma = &mut mapping.curves[0];
    luma.num_pivots = MAX_PIECES as u8 + 1;
    for (i, pivot) in luma.pivots.iter_mut().enumerate() {
        *pivot = 64 + 96 * i as u16;
    }
    // Each piece its own: a step up, and bending either way.
    for i in 0..MAX_PIECES {
        luma.mapping_idc[i] = 0;
        luma.poly_order[i] = 2;
        let bend = if i % 2 == 0 { 0.1 } else { -0.1 };
        luma.poly_coef[i] = [fixed(0.02 * i as f64), fixed(0.8), fixed(bend)];
    }
    for (c, chroma) in mapping.curves[1..].iter_mut().enumerate() {
        chroma.mapping_idc[0] = sys::MAPPING_MMR;
        chroma.mmr_order[0] = 3;
        chroma.mmr_constant[0] = one / 2;
        // Every weight its own, either way.
        for (k, power) in chroma.mmr_coef[0].iter_mut().enumerate() {
            for (i, weight) in power.iter_mut().enumerate() {
                let at = (c * 21 + k * 7 + i) as i64;
                *weight = if at % 2 == 0 { 1 } else { -1 } * one / (8 + at);
            }
        }
    }
    let reshaping = Reshaping::new(&header, &mapping, &color).unwrap();
    let params = reshaping.shader_params();
    let lumas = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.75, 0.95];
    for (at, luma) in lumas.into_iter().enumerate() {
        let samples = [luma, 0.3 + 0.04 * at as f32, 0.7 - 0.05 * at as f32];
        let cpu = reshaping.lms_to_rgb(reshaping.lms_signal(samples).map(pq_to_nits));
        let gpu = params.light(samples);
        for (a, b) in cpu.iter().zip(gpu) {
            assert!(
                (a - b).abs() <= 1e-3 * a.max(1.0),
                "{samples:?}: {cpu:?} {gpu:?}"
            );
        }
    }
    // An order said as none is the first.
    mapping.curves[1].mmr_order[0] = 0;
    let first = Reshaping::new(&header, &mapping, &color).unwrap();
    let Piece::Mmr { order, .. } = first.curves[1].pieces[0] else {
        panic!("MMR");
    };
    assert_eq!(order, 1);
}

/// `DoviParams` in gpui's Metal shader takes the params as they're packed:
/// as many `float4`s, in the same order.
#[test]
fn shader_params_are_laid_out_as_the_shader_reads_them() {
    let metal = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/gpui/src/platform/mac/shaders.metal"
    ));
    let declared = |name: &str| {
        let start = metal
            .find(&format!("struct {name} {{"))
            .unwrap_or_else(|| panic!("no {name}"));
        let body = &metal[start..];
        let fields: Vec<String> = body[body.find('{').unwrap() + 1..body.find("};").unwrap()]
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .map(str::to_owned)
            .collect();
        fields
    };
    assert_eq!(declared("DoviPiece"), ["float4 poly;", "float4 mmr[6];"]);
    assert_eq!(
        declared("DoviCurve"),
        [
            "float4 bounds;".to_owned(),
            "float4 pivots[2];".to_owned(),
            format!("DoviPiece pieces[{MAX_PIECES}];"),
        ]
    );
    assert_eq!(
        declared("DoviParams"),
        [
            "float4 ipt_to_lms[3];",
            "float4 lms_to_rgb[3];",
            "DoviCurve curves[3];"
        ]
    );
    let float4 = size_of::<[f32; 4]>();
    assert_eq!(size_of::<ShaderPiece>(), 7 * float4);
    assert_eq!(size_of::<ShaderCurve>(), (3 + MAX_PIECES * 7) * float4);
    assert_eq!(
        size_of::<ShaderParams>(),
        6 * float4 + 3 * size_of::<ShaderCurve>()
    );
}
