//! Side data read as the struct FFmpeg keeps it as, checked to be as big as
//! one first: a stream's coded side data with [`coded`], a frame's with
//! [`frame`], a frame's Dolby Vision RPU with [`rpu`].

use std::ptr;

use ffmpeg_next::{codec, ffi, frame};

/// FFmpeg's structs in libavutil/mastering_display_metadata.h and
/// libavutil/dovi_meta.h, the parts Plays reads, which the bindings don't
/// cover. Later FFmpeg versions only add fields at the ends, and
/// `AVDOVIMetadata` says where each of its structs starts.
pub mod sys {
    use std::ffi::{c_int, c_uint};

    use ffmpeg_next::ffi::AVRational;

    /// `AV_DOVI_MAX_PIECES`.
    pub const MAX_PIECES: usize = 8;

    /// `AVMasteringDisplayMetadata`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MasteringDisplay {
        pub display_primaries: [[AVRational; 2]; 3],
        pub white_point: [AVRational; 2],
        pub min_luminance: AVRational,
        pub max_luminance: AVRational,
        pub has_primaries: c_int,
        pub has_luminance: c_int,
    }

    /// `AVContentLightMetadata`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct ContentLightLevel {
        pub max_cll: c_uint,
        pub max_fall: c_uint,
    }

    /// `AVDOVIDecoderConfigurationRecord`, a stream's coded side data.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct DoviConfiguration {
        pub dv_version_major: u8,
        pub dv_version_minor: u8,
        pub dv_profile: u8,
        pub dv_level: u8,
        pub rpu_present_flag: u8,
        pub el_present_flag: u8,
        pub bl_present_flag: u8,
        /// What the base layer is without its RPU: 0 nothing shown alone
        /// (IPT), 1 and 6 HDR10, 2 SDR, 4 HLG.
        pub dv_bl_signal_compatibility_id: u8,
        pub dv_md_compression: u8,
    }

    /// `AVDOVIMetadata`: where in its side data each part starts.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct DoviMetadata {
        pub header_offset: usize,
        pub mapping_offset: usize,
        pub color_offset: usize,
    }

    /// `AVDOVIRpuDataHeader`.
    #[repr(C)]
    pub struct RpuDataHeader {
        pub rpu_type: u8,
        pub rpu_format: u16,
        pub vdr_rpu_profile: u8,
        pub vdr_rpu_level: u8,
        pub chroma_resampling_explicit_filter_flag: u8,
        pub coef_data_type: u8,
        pub coef_log2_denom: u8,
        pub vdr_rpu_normalized_idc: u8,
        pub bl_video_full_range_flag: u8,
        pub bl_bit_depth: u8,
        pub el_bit_depth: u8,
        pub vdr_bit_depth: u8,
        pub spatial_resampling_filter_flag: u8,
        pub el_spatial_resampling_filter_flag: u8,
        pub disable_residual_flag: u8,
    }

    /// `AV_DOVI_MAPPING_MMR`; the other method is polynomial.
    pub const MAPPING_MMR: i32 = 1;

    /// `AVDOVIReshapingCurve`.
    #[repr(C)]
    pub struct ReshapingCurve {
        pub num_pivots: u8,
        pub pivots: [u16; MAX_PIECES + 1],
        pub mapping_idc: [i32; MAX_PIECES],
        pub poly_order: [u8; MAX_PIECES],
        pub poly_coef: [[i64; 3]; MAX_PIECES],
        pub mmr_order: [u8; MAX_PIECES],
        pub mmr_constant: [i64; MAX_PIECES],
        pub mmr_coef: [[[i64; 7]; 3]; MAX_PIECES],
    }

    /// `AV_DOVI_NLQ_LINEAR_DZ`, the one method of dequantizing.
    pub const NLQ_LINEAR_DZ: i32 = 0;

    /// `AVDOVINLQParams`: how a component's enhancement layer is
    /// dequantized, in fixed point with `coef_log2_denom` bits of fraction.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct NlqParams {
        pub nlq_offset: u16,
        pub vdr_in_max: u64,
        pub linear_deadzone_slope: u64,
        pub linear_deadzone_threshold: u64,
    }

    /// `AVDOVIDataMapping`.
    #[repr(C)]
    pub struct DataMapping {
        pub vdr_rpu_id: u8,
        pub mapping_color_space: u8,
        pub mapping_chroma_format_idc: u8,
        pub curves: [ReshapingCurve; 3],
        pub nlq_method_idc: i32,
        pub num_x_partitions: u32,
        pub num_y_partitions: u32,
        pub nlq: [NlqParams; 3],
        pub nlq_pivots: [u16; 2],
    }

    /// `AVDOVIColorMetadata`.
    #[repr(C)]
    pub struct ColorMetadata {
        pub dm_metadata_id: u8,
        pub scene_refresh_flag: u8,
        pub ycc_to_rgb_matrix: [AVRational; 9],
        pub ycc_to_rgb_offset: [AVRational; 3],
        pub rgb_to_lms_matrix: [AVRational; 9],
        pub signal_eotf: u16,
        pub signal_eotf_param0: u16,
        pub signal_eotf_param1: u16,
        pub signal_eotf_param2: u32,
        pub signal_bit_depth: u8,
        pub signal_color_space: u8,
        pub signal_chroma_format: u8,
        pub signal_full_range_flag: u8,
        /// The mastering display's black and peak, as 12-bit PQ codes.
        pub source_min_pq: u16,
        pub source_max_pq: u16,
        pub source_diagonal: u16,
    }
}

/// A struct FFmpeg keeps a stream's coded side data of `KIND` as.
///
/// # Safety
///
/// Side data of `KIND` must begin as a `Self`, and any bytes must make one:
/// it holds only numbers.
pub unsafe trait Coded: Copy {
    const KIND: ffi::AVPacketSideDataType;
}

/// A struct FFmpeg keeps a frame's side data of `KIND` as.
///
/// # Safety
///
/// As for [`Coded`].
pub unsafe trait Framed {
    const KIND: ffi::AVFrameSideDataType;
}

// SAFETY: each is all numbers, and FFmpeg keeps the side data named as it.
unsafe impl Coded for sys::MasteringDisplay {
    const KIND: ffi::AVPacketSideDataType =
        ffi::AVPacketSideDataType::AV_PKT_DATA_MASTERING_DISPLAY_METADATA;
}
// SAFETY: as above.
unsafe impl Coded for sys::ContentLightLevel {
    const KIND: ffi::AVPacketSideDataType =
        ffi::AVPacketSideDataType::AV_PKT_DATA_CONTENT_LIGHT_LEVEL;
}
// SAFETY: as above.
unsafe impl Coded for sys::DoviConfiguration {
    const KIND: ffi::AVPacketSideDataType = ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF;
}
// SAFETY: as above.
unsafe impl Coded for DisplayMatrix {
    const KIND: ffi::AVPacketSideDataType = ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX;
}
// SAFETY: as above.
unsafe impl Framed for sys::MasteringDisplay {
    const KIND: ffi::AVFrameSideDataType =
        ffi::AVFrameSideDataType::AV_FRAME_DATA_MASTERING_DISPLAY_METADATA;
}
// SAFETY: as above.
unsafe impl Framed for sys::ContentLightLevel {
    const KIND: ffi::AVFrameSideDataType =
        ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL;
}

/// `parameters`' side data of `kind`, or null.
fn coded_side(
    parameters: &codec::Parameters,
    kind: ffi::AVPacketSideDataType,
) -> *const ffi::AVPacketSideData {
    // SAFETY: the parameters are valid while borrowed, and FFmpeg looks
    // only within their side data's count.
    unsafe {
        let raw = parameters.as_ptr();
        ffi::av_packet_side_data_get((*raw).coded_side_data, (*raw).nb_coded_side_data, kind)
    }
}

/// A copy of `parameters`' side data of `T`'s kind, if they have some as
/// big as a `T`.
pub fn coded<T: Coded>(parameters: &codec::Parameters) -> Option<T> {
    let side = coded_side(parameters, T::KIND);
    // SAFETY: side data stays valid while the parameters do; its size is
    // checked before it's read, unaligned, and any bytes make a `T`.
    unsafe {
        if side.is_null() || (*side).data.is_null() || (*side).size < size_of::<T>() {
            return None;
        }
        Some(ptr::read_unaligned((*side).data.cast::<T>()))
    }
}

/// Whether `parameters` have side data of `T`'s kind, as big as a `T` or
/// cut short.
pub fn has_coded<T: Coded>(parameters: &codec::Parameters) -> bool {
    !coded_side(parameters, T::KIND).is_null()
}

/// `frame`'s side data of `T`'s kind, if it has some as big as a `T`, and
/// placed as one must be.
pub fn frame<T: Framed>(frame: &frame::Video) -> Option<&T> {
    // SAFETY: side data stays valid as long as the frame, which the
    // reference borrows; its size and place are checked before it's read,
    // and any bytes make a `T`.
    unsafe {
        let side = ffi::av_frame_get_side_data(frame.as_ptr(), T::KIND);
        if side.is_null() {
            return None;
        }
        part((*side).data.cast_const(), (*side).size, 0)
    }
}

/// The `T` at `offset` in `size` bytes at `data`, if it's within them and
/// placed as a `T` must be.
///
/// # Safety
///
/// `data`, unless null, must be `size` bytes that stay as they are for
/// `'a`, and any bytes must make a `T`.
unsafe fn part<'a, T>(data: *const u8, size: usize, offset: usize) -> Option<&'a T> {
    let within = offset
        .checked_add(size_of::<T>())
        .is_some_and(|end| end <= size);
    if data.is_null() || !within {
        return None;
    }
    // SAFETY: within the bytes, as checked.
    let at = unsafe { data.add(offset) };
    // SAFETY: as the caller says, and placed as a `T`, as checked.
    at.cast::<T>()
        .is_aligned()
        .then(|| unsafe { &*at.cast::<T>() })
}

/// A frame's Dolby Vision RPU as FFmpeg parsed it.
#[derive(Clone, Copy)]
pub struct Rpu<'a> {
    pub header: &'a sys::RpuDataHeader,
    pub mapping: &'a sys::DataMapping,
    pub color: &'a sys::ColorMetadata,
}

/// `frame`'s Dolby Vision RPU, if it has one each of whose parts is within
/// its side data.
pub fn rpu(frame: &frame::Video) -> Option<Rpu<'_>> {
    // SAFETY: side data stays valid as long as the frame, which the
    // references borrow; each part is checked to be within it, and placed
    // as it must be, before it is read, and each is all numbers.
    unsafe {
        let side = ffi::av_frame_get_side_data(
            frame.as_ptr(),
            ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_METADATA,
        );
        if side.is_null() {
            return None;
        }
        let (data, size) = ((*side).data.cast_const(), (*side).size);
        let metadata = part::<sys::DoviMetadata>(data, size, 0)?;
        Some(Rpu {
            header: part(data, size, metadata.header_offset)?,
            mapping: part(data, size, metadata.mapping_offset)?,
            color: part(data, size, metadata.color_offset)?,
        })
    }
}

/// FFmpeg's display matrix, of 16.16 fixed point numbers but for its last
/// column's 2.30. It takes a pixel (p, q) to (a p + c q, b p + d q), with
/// a, b, c, d its first two columns of its first two rows.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMatrix(pub [i32; 9]);

impl DisplayMatrix {
    /// The quarter turns clockwise, after mirroring left to right if so,
    /// nearest to what it asks. `None` where it squashes the picture flat.
    pub fn turns(&self) -> Option<(u8, bool)> {
        let fixed = |i: usize| f64::from(self.0[i]) / 65536.0;
        let wanted = [[fixed(0), fixed(3)], [fixed(1), fixed(4)]];
        if (wanted[0][0] * wanted[1][1] - wanted[0][1] * wanted[1][0]).abs() < 1e-6 {
            return None;
        }
        let likeness = |way: (u8, bool)| {
            let taken = takes(way);
            (0..2)
                .flat_map(|row| (0..2).map(move |column| (row, column)))
                .map(|(row, column)| taken[row][column] * wanted[row][column])
                .sum::<f64>()
        };
        (0..8)
            .map(|way| (way % 4, way >= 4))
            .max_by(|a, b| likeness(*a).total_cmp(&likeness(*b)))
    }
}

/// Where `turns` quarter turns clockwise, after mirroring if `mirrored`,
/// take a point, as `[[xx, xy], [yx, yy]]` (`x' = xx x + xy y`), with x to
/// the right and y down.
fn takes((turns, mirrored): (u8, bool)) -> [[f64; 2]; 2] {
    let sign = if mirrored { -1.0 } else { 1.0 };
    // A quarter turn clockwise takes (x, y) to (-y, x); mirroring first
    // negates x.
    match turns {
        0 => [[sign, 0.0], [0.0, 1.0]],
        1 => [[0.0, -1.0], [sign, 0.0]],
        2 => [[-sign, 0.0], [0.0, -1.0]],
        _ => [[0.0, 1.0], [-sign, 0.0]],
    }
}

/// What a stream's Dolby Vision configuration record says it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DolbyVision {
    pub profile: u8,
    pub level: u8,
    /// What its base layer shows as without Dolby Vision: 0 nothing else
    /// can show it (IPT), 1 HDR10, 2 SDR, 4 HLG, 6 a Blu-ray's HDR10.
    pub compatibility: u8,
    /// Its frames carry RPUs.
    pub rpu: bool,
    /// The second layer profile 7 adds detail with.
    pub enhancement: bool,
    /// The base layer: the picture players without Dolby Vision show.
    pub base: bool,
}

impl DolbyVision {
    /// What `parameters`' configuration record says, if they have a whole
    /// one.
    pub fn of(parameters: &codec::Parameters) -> Option<Self> {
        let record = coded::<sys::DoviConfiguration>(parameters)?;
        Some(Self {
            profile: record.dv_profile,
            level: record.dv_level,
            compatibility: record.dv_bl_signal_compatibility_id,
            rpu: record.rpu_present_flag != 0,
            enhancement: record.el_present_flag != 0,
            base: record.bl_present_flag != 0,
        })
    }

    /// Whether `parameters` have a configuration record, whole or cut
    /// short.
    pub fn is_tagged(parameters: &codec::Parameters) -> bool {
        has_coded::<sys::DoviConfiguration>(parameters)
    }

    /// Only an enhancement layer: a track of its own beside the base
    /// layer's, which never shows alone.
    pub fn is_layer_only(self) -> bool {
        self.enhancement && !self.base
    }
}

#[cfg(test)]
#[path = "tests/side_data.rs"]
mod tests;
