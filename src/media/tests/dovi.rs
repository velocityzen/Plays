use super::*;
use crate::media::hdr::{REFERENCE_WHITE_NITS, Transfer, pq_to_nits};
use std::mem::offset_of;

fn rational(num: i32, den: i32) -> ffi::AVRational {
    ffi::AVRational { num, den }
}

/// The first frame of a profile 5 stream, as ffprobe prints it: the
/// default IPTPQc2 matrices, a polynomial for luma and a straight line
/// for each chroma component.
fn profile5() -> (
    sys::RpuDataHeader,
    Box<sys::DataMapping>,
    sys::ColorMetadata,
) {
    // SAFETY: all-zero is valid for these plain structs.
    let mut header: sys::RpuDataHeader = unsafe { std::mem::zeroed() };
    header.coef_log2_denom = 23;
    header.bl_bit_depth = 10;
    // SAFETY: as for the header.
    let mut mapping: Box<sys::DataMapping> = Box::new(unsafe { std::mem::zeroed() });
    let luma = &mut mapping.curves[0];
    luma.num_pivots = 3;
    luma.pivots[..3].copy_from_slice(&[0, 512, 1023]);
    luma.poly_order[..2].copy_from_slice(&[1, 2]);
    luma.poly_coef[0] = [0, 1 << 23, 0];
    luma.poly_coef[1] = [1 << 21, 0, 1 << 22];
    for chroma in &mut mapping.curves[1..] {
        chroma.num_pivots = 2;
        chroma.pivots[..2].copy_from_slice(&[0, 1023]);
        chroma.poly_order[0] = 1;
        chroma.poly_coef[0] = [-649_193, 10_699_938, 0];
    }
    let m = |values: [i32; 9], den| values.map(|v| rational(v, den));
    let color = sys::ColorMetadata {
        dm_metadata_id: 0,
        scene_refresh_flag: 1,
        ycc_to_rgb_matrix: m([8192, 799, 1681, 8192, -933, 1091, 8192, 267, -5545], 8192),
        ycc_to_rgb_offset: [0, 1 << 27, 1 << 27].map(|v| rational(v, 1 << 28)),
        rgb_to_lms_matrix: m(
            [17081, -349, -349, -349, 17081, -349, -349, -349, 17081],
            16384,
        ),
        signal_eotf: 65535,
        signal_eotf_param0: 0,
        signal_eotf_param1: 0,
        signal_eotf_param2: 0,
        signal_bit_depth: 12,
        signal_color_space: 0,
        signal_chroma_format: 0,
        signal_full_range_flag: 1,
        // 0.005 and 1,000 nits.
        source_min_pq: 62,
        source_max_pq: 3079,
        source_diagonal: 42,
    };
    (header, mapping, color)
}

pub(crate) fn reshaping() -> Reshaping {
    let (header, mapping, color) = profile5();
    Reshaping::new(&header, &mapping, &color).unwrap()
}

/// A profile 5 frame of one color all over, its luma and chroma `codes`
/// (10-bit), with `profile5`'s RPU as side data, mastered on a display
/// whose peak is `source_max_pq`.
pub(crate) fn rpu_frame(size: (u32, u32), codes: [u16; 3], source_max_pq: u16) -> frame::Video {
    use ffmpeg::util::format::Pixel;

    let (header, mapping, mut color) = profile5();
    color.source_max_pq = source_max_pq;
    let mut frame = frame::Video::new(Pixel::P010LE, size.0, size.1);
    for (plane, codes) in [(0, &codes[..1]), (1, &codes[1..])] {
        let stride = frame.stride(plane);
        for row in frame.data_mut(plane).chunks_exact_mut(stride) {
            for (x, sample) in row.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                *sample = (codes[x % codes.len()] << 6).to_le_bytes();
            }
        }
    }
    attach_rpu(&mut frame, &header, &mapping, &color);
    frame
}

/// Give `frame` an RPU of these parts, as FFmpeg's side data has it.
pub(crate) fn attach_rpu(
    frame: &mut frame::Video,
    header: &sys::RpuDataHeader,
    mapping: &sys::DataMapping,
    color: &sys::ColorMetadata,
) {
    let after = |offset: usize, len: usize| (offset + len).next_multiple_of(8);
    let header_offset = after(0, size_of::<sys::Metadata>());
    let mapping_offset = after(header_offset, size_of::<sys::RpuDataHeader>());
    let color_offset = after(mapping_offset, size_of::<sys::DataMapping>());
    let len = color_offset + size_of::<sys::ColorMetadata>();
    // SAFETY: FFmpeg makes side data `len` bytes long, aligned for any
    // struct, and each goes within it at a multiple of 8, copied from one
    // of its own type.
    unsafe {
        let side = ffi::av_frame_new_side_data(
            frame.as_mut_ptr(),
            ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_METADATA,
            len,
        );
        assert!(!side.is_null());
        let data = (*side).data;
        data.cast::<sys::Metadata>().write(sys::Metadata {
            header_offset,
            mapping_offset,
            color_offset,
        });
        std::ptr::copy_nonoverlapping(header, data.add(header_offset).cast(), 1);
        std::ptr::copy_nonoverlapping(mapping, data.add(mapping_offset).cast(), 1);
        std::ptr::copy_nonoverlapping(color, data.add(color_offset).cast(), 1);
    }
}

#[test]
fn mirrors_ffmpegs_layout() {
    // From offsetof() against libavutil/dovi_meta.h.
    assert_eq!(offset_of!(sys::RpuDataHeader, coef_log2_denom), 8);
    assert_eq!(offset_of!(sys::RpuDataHeader, bl_bit_depth), 11);
    assert_eq!(offset_of!(sys::RpuDataHeader, disable_residual_flag), 16);
    assert_eq!(size_of::<sys::ReshapingCurve>(), 1672);
    assert_eq!(offset_of!(sys::ReshapingCurve, mapping_idc), 20);
    assert_eq!(offset_of!(sys::ReshapingCurve, poly_coef), 64);
    assert_eq!(offset_of!(sys::ReshapingCurve, mmr_constant), 264);
    assert_eq!(offset_of!(sys::ReshapingCurve, mmr_coef), 328);
    assert_eq!(offset_of!(sys::DataMapping, curves), 8);
    assert_eq!(offset_of!(sys::DataMapping, nlq_method_idc), 5024);
    assert_eq!(offset_of!(sys::DataMapping, num_x_partitions), 5028);
    assert_eq!(offset_of!(sys::DataMapping, num_y_partitions), 5032);
    assert_eq!(offset_of!(sys::DataMapping, nlq), 5040);
    assert_eq!(offset_of!(sys::DataMapping, nlq_pivots), 5136);
    assert_eq!(size_of::<sys::DataMapping>(), 5144);
    assert_eq!(size_of::<sys::NlqParams>(), 32);
    assert_eq!(offset_of!(sys::NlqParams, vdr_in_max), 8);
    assert_eq!(offset_of!(sys::NlqParams, linear_deadzone_threshold), 24);
    assert_eq!(offset_of!(sys::Configuration, dv_profile), 2);
    assert_eq!(offset_of!(sys::Configuration, el_present_flag), 5);
    assert_eq!(offset_of!(sys::Configuration, bl_present_flag), 6);
    assert_eq!(size_of::<sys::Configuration>(), 9);
    assert_eq!(offset_of!(sys::ColorMetadata, ycc_to_rgb_offset), 76);
    assert_eq!(offset_of!(sys::ColorMetadata, rgb_to_lms_matrix), 100);
    assert_eq!(offset_of!(sys::ColorMetadata, signal_eotf_param2), 180);
    assert_eq!(offset_of!(sys::ColorMetadata, source_max_pq), 190);
    assert_eq!(size_of::<sys::ColorMetadata>(), 196);
    // `DOVI_PARAMS_SIZE` in vendor/gpui's metal_renderer.rs, which is
    // within Metal's limit of 4 KB for bytes set straight on an encoder.
    #[cfg(target_os = "macos")]
    assert_eq!(size_of::<ShaderParams>(), 2928);
}

#[test]
fn reads_no_rpu_past_the_end_of_its_side_data() {
    use ffmpeg::util::format::Pixel;

    let (header, mapping, color) = profile5();
    // The mapping last, where no part after it vouches for its end.
    let after = |offset: usize, len: usize| (offset + len).next_multiple_of(8);
    let header_offset = after(0, size_of::<sys::Metadata>());
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
            data.cast::<sys::Metadata>().write(sys::Metadata {
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
            rpu(&frame).is_some(),
            len == whole,
            "{len} of {whole} bytes"
        );
    }
}

#[test]
fn the_mastering_displays_peak_is_the_videos() {
    let peak = reshaping().peak_nits().unwrap();
    assert!((peak - 1000.6).abs() < 0.1, "{peak}");
    let (header, mapping, mut color) = profile5();
    color.source_max_pq = 0;
    let unsaid = Reshaping::new(&header, &mapping, &color).unwrap();
    assert_eq!(unsaid.peak_nits(), None);
    // As a frame's RPU says.
    let frame = rpu_frame((4, 2), [512; 3], 3696);
    let peak = Reshaping::of(&frame).unwrap().peak_nits().unwrap();
    assert!((peak - 4000.0).abs() < 5.0, "{peak}");
}

#[test]
fn picks_pieces_between_pivots() {
    let luma = &reshaping().curves[0];
    let at = |code: f32| luma.apply(code / 1024.0, [code / 1024.0; 3]);
    // The straight first piece, then x² / 2 + 1/4 from code 512 on.
    assert!((at(256.0) - 0.25).abs() < 1e-6);
    assert!((at(511.0) - 511.0 / 1024.0).abs() < 1e-6);
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
    // The code where this frame's chroma reshapes to exactly neutral.
    let neutral = (0.5 + 649_193.0 / 8_388_608.0) / (10_699_938.0 / 8_388_608.0);
    let lms = reshaping.lms_signal([0.4, neutral, neutral]);
    assert!(lms.iter().all(|&s| (s - 0.4).abs() < 1e-5), "{lms:?}");
    let rgb = reshaping.lms_to_rgb(lms.map(pq_to_nits));
    let nits = pq_to_nits(0.4);
    assert!(
        rgb.iter().all(|&c| (c / nits - 1.0).abs() < 1e-3),
        "{rgb:?}"
    );
}

#[test]
#[cfg(target_os = "macos")]
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
}

#[test]
fn renders_planes_through_the_tone_mapper() {
    let reshaping = reshaping();
    let mapper = ToneMapper::new(Transfer::Pq);
    // One pixel, neutral, that reshapes to PQ's reference white (in the
    // second piece of the luma curve, x² / 2 + 1/4).
    let neutral = (0.5 + 649_193.0 / 8_388_608.0) / (10_699_938.0 / 8_388_608.0);
    let white = 0.5806f32;
    let plane = |v: f32| ((v * 65536.0).round() as u16).to_le_bytes().to_vec();
    let (y, c) = (plane(((white - 0.25) * 2.0).sqrt()), plane(neutral));
    let out = reshaping.render(&mapper, [(&y, 2), (&c, 2), (&c, 2)], 1, 1);
    let [b, g, r, a] = out[..4] else {
        unreachable!()
    };
    assert_eq!(a, 255);
    assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1, "{out:?}");
    // Reference white comes out just under SDR white, as for HDR10.
    assert!((235..=250).contains(&g), "{out:?}");
    assert!((pq_to_nits(white) - REFERENCE_WHITE_NITS).abs() < 1.0);
}

/// Render 10-bit base layer codes (I, P, T per pixel, row by row) as the
/// decoder hands them over: in the top bits of 16-bit planes whose rows
/// are padded with junk. Returns RGB per pixel.
fn render_codes(codes: &[[u16; 3]], width: usize) -> Vec<[u8; 3]> {
    let height = codes.len() / width;
    let stride = width * 2 + 6;
    let planes: [Vec<u8>; 3] = std::array::from_fn(|c| {
        let mut plane = vec![0xff; stride * height];
        for (i, pixel) in codes.iter().enumerate() {
            let at = (i / width) * stride + (i % width) * 2;
            plane[at..at + 2].copy_from_slice(&(pixel[c] << 6).to_le_bytes());
        }
        plane
    });
    let mapper = ToneMapper::new(Transfer::Pq);
    let planes = [0, 1, 2].map(|c| (planes[c].as_slice(), stride));
    let out = reshaping().render(&mapper, planes, width, height);
    let (pixels, _) = out.as_chunks::<4>();
    assert!(pixels.iter().all(|&[.., a]| a == 255));
    pixels.iter().map(|&[b, g, r, _]| [r, g, b]).collect()
}

#[test]
fn renders_a_known_frame() {
    // Colors put into IPTPQc2 and through this frame's curves backwards
    // to 10-bit codes, the way an encoder makes a profile 5 picture. The
    // bytes they should come back as were worked out apart from this
    // code, from the standards (ST 2084, the IPTPQc2 matrices, BT.2087,
    // sRGB) and the tone curve, for the codes as rounded; where the
    // rounding moves a color, it's by a step or two.
    let frame: [([u16; 3], [u8; 3]); 8] = [
        // Black.
        ([0, 464, 464], [0, 0, 0]),
        // Reference white, just under SDR white as for HDR10.
        ([833, 464, 464], [245, 244, 243]),
        // Grey at 20 nits.
        ([366, 464, 464], [89, 89, 88]),
        // A skin tone, BT.709 (60, 40, 30) nits.
        ([432, 481, 492], [148, 123, 107]),
        // BT.709 red at 100 nits, as HDR10 shows it.
        ([341, 632, 623], [186, 0, 0]),
        // BT.709 green at 50 nits.
        ([387, 365, 588], [4, 136, 1]),
        // BT.709 blue at 20 nits.
        ([214, 418, 304], [0, 0, 88]),
        // Yellow at 400 nits, well over SDR white.
        ([866, 423, 632], [255, 255, 0]),
    ];
    let codes: Vec<_> = frame.iter().map(|(codes, _)| *codes).collect();
    let rendered = render_codes(&codes, 4);
    for ((codes, want), got) in frame.iter().zip(rendered) {
        let close = want.iter().zip(got).all(|(&a, b)| a.abs_diff(b) <= 1);
        assert!(close, "{codes:?}: want {want:?}, got {got:?}");
    }
}

#[test]
fn chroma_tints_the_way_ipt_says() {
    // Reference white grey, then its P (red against green) and T (yellow
    // against blue) codes moved either way.
    let [grey, more_p, less_p, more_t, less_t] = render_codes(
        &[
            [833, 464, 464],
            [833, 504, 464],
            [833, 424, 464],
            [833, 464, 504],
            [833, 464, 424],
        ],
        5,
    )[..] else {
        unreachable!()
    };
    let [r, g, b] = grey.map(i32::from);
    assert!((r - g).abs() <= 2 && (g - b).abs() <= 2, "{grey:?}");
    let most = |c: [u8; 3]| (0..3).max_by_key(|&i| c[i]).unwrap();
    let least = |c: [u8; 3]| (0..3).min_by_key(|&i| c[i]).unwrap();
    assert_eq!((most(more_p), least(more_p)), (0, 1), "redder: {more_p:?}");
    assert_eq!((most(less_p), least(less_p)), (1, 0), "greener: {less_p:?}");
    assert_eq!(least(more_t), 2, "yellower: {more_t:?}");
    assert_eq!(most(less_t), 2, "bluer: {less_t:?}");
}
