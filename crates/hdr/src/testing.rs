//! What tests make HDR frames from: profile 5's RPU as a stream's first
//! frame has it, and HDR10's metadata, as FFmpeg's side data keeps them.

use ffmpeg_next::{ffi, frame};
use ffmpeg_safe::sys;

use crate::Reshaping;

fn rational(num: i32, den: i32) -> ffi::AVRational {
    ffi::AVRational { num, den }
}

/// The first frame of a profile 5 stream, as ffprobe prints it: the
/// default IPTPQc2 matrices, a polynomial for luma and a straight line
/// for each chroma component, mastered at 1,000 nits.
pub fn profile5() -> (
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

/// `profile5`'s reshaping.
pub fn reshaping() -> Reshaping {
    let (header, mapping, color) = profile5();
    Reshaping::new(&header, &mapping, &color).expect("profile 5's RPU reshapes")
}

/// The chroma signal (a code over 2^10) `profile5`'s curves reshape to
/// exactly neutral.
pub const NEUTRAL: f32 = (0.5 + 649_193.0 / 8_388_608.0) / (10_699_938.0 / 8_388_608.0);

/// Give `frame` an RPU of these parts, as FFmpeg's side data has it.
pub fn attach_rpu(
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

/// Give `frame` side data of a MaxCLL and of a mastering display's peak
/// (`num` / `den` nits, and whether it says), each if given.
pub fn attach_hdr10(
    frame: &mut frame::Video,
    max_cll: Option<u32>,
    display: Option<(i32, i32, bool)>,
) {
    let mut side = |kind, size| {
        // SAFETY: FFmpeg makes side data of the size asked for, or none.
        let side = unsafe { ffi::av_frame_new_side_data(frame.as_mut_ptr(), kind, size) };
        assert!(!side.is_null());
        // SAFETY: as above.
        unsafe { (*side).data }
    };
    if let Some(max_cll) = max_cll {
        let data = side(
            ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL,
            size_of::<sys::ContentLightLevel>(),
        );
        // SAFETY: the side data is as big as the struct, and aligned.
        unsafe {
            data.cast::<sys::ContentLightLevel>()
                .write(sys::ContentLightLevel {
                    max_cll,
                    max_fall: max_cll / 2,
                });
        }
    }
    if let Some((num, den, has_luminance)) = display {
        let data = side(
            ffi::AVFrameSideDataType::AV_FRAME_DATA_MASTERING_DISPLAY_METADATA,
            size_of::<sys::MasteringDisplay>(),
        );
        // SAFETY: as above; all-zero is a valid display otherwise.
        unsafe {
            let mut mastering: sys::MasteringDisplay = std::mem::zeroed();
            mastering.max_luminance = ffi::AVRational { num, den };
            mastering.min_luminance = ffi::AVRational {
                num: 50,
                den: 10_000,
            };
            mastering.has_luminance = has_luminance.into();
            data.cast::<sys::MasteringDisplay>().write(mastering);
        }
    }
}
