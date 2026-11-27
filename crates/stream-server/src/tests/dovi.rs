use super::*;
use crate::hdr::pq_to_nits;

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

fn reshaping() -> Reshaping {
    let (header, mapping, color) = profile5();
    Reshaping::new(&header, &mapping, &color).unwrap()
}

/// The 10-bit chroma code this frame's curves reshape to exactly neutral.
pub(crate) const NEUTRAL: f32 = (0.5 + 649_193.0 / 8_388_608.0) / (10_699_938.0 / 8_388_608.0);

/// A 64 × 32 profile 5 frame, 10-bit in full range as such a picture is,
/// of one color all over: its luma and chroma `codes`, with `profile5`'s
/// RPU as side data.
pub(crate) fn rpu_frame(codes: [u16; 3]) -> frame::Video {
    use ffmpeg::util::color::Range;
    use ffmpeg::util::format::Pixel;

    let (header, mapping, color) = profile5();
    let mut frame = frame::Video::new(Pixel::YUV420P10LE, 64, 32);
    frame.set_color_range(Range::JPEG);
    for (plane, code) in codes.into_iter().enumerate() {
        let (width, rows) = (
            frame.plane_width(plane) as usize,
            frame.plane_height(plane) as usize,
        );
        let (data, stride) = ffmpeg_safe::plane_mut(&mut frame, plane).unwrap();
        for row in data.chunks_mut(stride).take(rows) {
            for sample in row[..width * 2].as_chunks_mut::<2>().0 {
                *sample = code.to_le_bytes();
            }
        }
    }
    attach_rpu(&mut frame, &header, &mapping, &color);
    frame
}

/// Give `frame` an RPU of these parts, as FFmpeg's side data has it.
fn attach_rpu(
    frame: &mut frame::Video,
    header: &sys::RpuDataHeader,
    mapping: &sys::DataMapping,
    color: &sys::ColorMetadata,
) {
    let after = |offset: usize, len: usize| (offset + len).next_multiple_of(8);
    let header_offset = after(0, size_of::<sys::DoviMetadata>());
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
        data.cast::<sys::DoviMetadata>().write(sys::DoviMetadata {
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
fn reads_no_rpu_past_the_end_of_its_side_data() {
    use ffmpeg::util::format::Pixel;

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
}

#[test]
fn the_mastering_displays_peak_is_the_videos() {
    let peak = reshaping().peak_nits().unwrap();
    assert!((peak - 1000.6).abs() < 0.1, "{peak}");
    let (header, mapping, mut color) = profile5();
    color.source_max_pq = 0;
    let unsaid = Reshaping::new(&header, &mapping, &color).unwrap();
    assert_eq!(unsaid.peak_nits(), None);
    assert!(Reshaping::of(&rpu_frame([512; 3])).is_some());
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
    let lms = reshaping.lms_signal([0.4, NEUTRAL, NEUTRAL]);
    assert!(lms.iter().all(|&s| (s - 0.4).abs() < 1e-5), "{lms:?}");
    let rgb = reshaping.lms_to_rgb(lms.map(pq_to_nits));
    let nits = pq_to_nits(0.4);
    assert!(
        rgb.iter().all(|&c| (c / nits - 1.0).abs() < 1e-3),
        "{rgb:?}"
    );
}
