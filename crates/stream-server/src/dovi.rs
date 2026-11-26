//! Dolby Vision whose pictures are IPT (profiles 5, 10.0 and 20), with no
//! HDR10, HLG or SDR picture to fall back on: they only look right reshaped
//! by the curves and matrices each frame's RPU carries, which FFmpeg parses
//! into `AV_FRAME_DATA_DOVI_METADATA`. Sent as YCbCr, without its
//! signalling, as a repackaged or re-encoded stream is, a picture turns
//! green and purple; a re-encoding reshapes it here first, as Plays does
//! (`src/media/dovi.rs`, whose math this mirrors: libplacebo's).
//!
//! Each component goes through its own piecewise curve, polynomials or MMR
//! (which mixes in the other two). Then a matrix turns IPT into PQ-encoded
//! LMS, and PQ's EOTF and a second matrix lead to linear BT.2020 light.

use ffmpeg::{ffi, frame};
use ffmpeg_next as ffmpeg;

use crate::hdr;

/// FFmpeg's `AV_DOVI_MAX_PIECES`.
const MAX_PIECES: usize = 8;

/// From BT.2020's Hunt-Pointer-Estevez LMS, without crosstalk, to BT.2020
/// RGB. The RPU's own linear matrix comes first (libplacebo's values).
const LMS_TO_BT2020: [[f32; 3]; 3] = [
    [3.064_396_8, -2.165_961_3, 0.101_557_41],
    [-0.656_131_3, 1.785_569_4, -0.129_439_5],
    [0.017_360_633, -0.047_244_53, 1.029_889_5],
];

/// The parts of FFmpeg's structs in libavutil/dovi_meta.h that are read
/// here, which the bindings don't cover. Later FFmpeg versions only add
/// fields at the ends, and `AVDOVIMetadata` says where each struct starts.
#[allow(dead_code)]
pub(crate) mod sys {
    use super::MAX_PIECES;
    use ffmpeg_next::ffi::AVRational;

    /// `AVDOVIDecoderConfigurationRecord`, a stream's coded side data.
    #[repr(C)]
    pub struct Configuration {
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

    #[repr(C)]
    pub struct Metadata {
        pub header_offset: usize,
        pub mapping_offset: usize,
        pub color_offset: usize,
    }

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

    /// `AVDOVINLQParams`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct NlqParams {
        pub nlq_offset: u16,
        pub vdr_in_max: u64,
        pub linear_deadzone_slope: u64,
        pub linear_deadzone_threshold: u64,
    }

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

/// A frame's RPU as FFmpeg parsed it, from its side data.
fn rpu(
    frame: &frame::Video,
) -> Option<(&sys::RpuDataHeader, &sys::DataMapping, &sys::ColorMetadata)> {
    // SAFETY: side data stays valid as long as the frame, which the
    // references borrow; each struct is checked to be within it before it
    // is read, and FFmpeg places each aligned for any struct.
    unsafe {
        let side = ffi::av_frame_get_side_data(
            frame.as_ptr(),
            ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_METADATA,
        );
        if side.is_null() {
            return None;
        }
        let (data, size) = ((*side).data as *const u8, (*side).size);
        let within =
            |offset: usize, len: usize| offset.checked_add(len).is_some_and(|end| end <= size);
        if data.is_null() || !within(0, size_of::<sys::Metadata>()) {
            return None;
        }
        let metadata = &*(data as *const sys::Metadata);
        let part = |offset: usize, len: usize| within(offset, len).then(|| data.add(offset));
        let header = part(metadata.header_offset, size_of::<sys::RpuDataHeader>())?;
        let mapping = part(metadata.mapping_offset, size_of::<sys::DataMapping>())?;
        let color = part(metadata.color_offset, size_of::<sys::ColorMetadata>())?;
        Some((
            &*(header as *const sys::RpuDataHeader),
            &*(mapping as *const sys::DataMapping),
            &*(color as *const sys::ColorMetadata),
        ))
    }
}

#[derive(Clone, Copy, Debug)]
enum Piece {
    /// Coefficients of x⁰, x¹ and x².
    Polynomial([f32; 3]),
    /// Multivariate multiple regression over all three components, up to
    /// the third power.
    Mmr {
        order: usize,
        constant: f32,
        /// Per power: the weights of s₀, s₁, s₂, s₀s₁, s₀s₂, s₁s₂, s₀s₁s₂.
        coefficients: [[f32; 7]; 3],
    },
}

/// One component's reshaping: pieces between pivots, in signals over
/// 2^bit depth.
#[derive(Clone, Debug)]
struct Curve {
    pivots: Vec<f32>,
    pieces: Vec<Piece>,
}

impl Curve {
    fn first(&self) -> f32 {
        self.pivots[0]
    }

    fn last(&self) -> f32 {
        self.pivots[self.pivots.len() - 1]
    }

    /// The piece for `s`, which is within the pivots.
    fn piece(&self, s: f32) -> &Piece {
        let inner = &self.pivots[1..self.pivots.len() - 1];
        let index = inner.iter().take_while(|&&pivot| s >= pivot).count();
        &self.pieces[index]
    }

    /// The reshaped component, from all three (clamped to their curves).
    fn apply(&self, own: f32, all: [f32; 3]) -> f32 {
        let value = match *self.piece(own) {
            Piece::Polynomial([c0, c1, c2]) => (c2 * own + c1) * own + c0,
            Piece::Mmr {
                order,
                constant,
                coefficients,
            } => {
                let [a, b, c] = all;
                let terms = [a, b, c, a * b, a * c, b * c, a * b * c];
                let mut power = terms;
                let mut value = constant;
                for (k, weights) in coefficients.iter().enumerate().take(order) {
                    if k > 0 {
                        power = std::array::from_fn(|i| power[i] * terms[i]);
                    }
                    value += weights.iter().zip(power).map(|(w, t)| w * t).sum::<f32>();
                }
                value
            }
        };
        value.clamp(0.0, 1.0)
    }
}

/// A frame's reshaping and matrices, from its RPU.
#[derive(Clone, Debug)]
pub(crate) struct Reshaping {
    curves: [Curve; 3],
    /// Rows from reshaped IPT to PQ-encoded LMS, with the RPU's offset
    /// folded into the last column.
    ipt_to_lms: [[f32; 4]; 3],
    /// Rows from linear LMS to linear BT.2020 RGB.
    lms_to_rgb: [[f32; 3]; 3],
    /// The peak of the display it was mastered on, in nits, if it says.
    peak_nits: Option<f32>,
}

impl Reshaping {
    /// The reshaping `frame`'s RPU gives, if it has one that can be read.
    pub fn of(frame: &frame::Video) -> Option<Self> {
        let (header, mapping, color) = rpu(frame)?;
        Self::new(header, mapping, color)
    }

    fn new(
        header: &sys::RpuDataHeader,
        mapping: &sys::DataMapping,
        color: &sys::ColorMetadata,
    ) -> Option<Self> {
        if !(8..=16).contains(&header.bl_bit_depth) || header.coef_log2_denom > 32 {
            return None;
        }
        let pivot_scale = 1.0 / f64::from(1u32 << header.bl_bit_depth);
        let coef_scale = 1.0 / (1u64 << header.coef_log2_denom) as f64;
        let coef = |c: i64| (c as f64 * coef_scale) as f32;
        let mut curves = Vec::with_capacity(3);
        for curve in &mapping.curves {
            let pivots = usize::from(curve.num_pivots);
            if !(2..=MAX_PIECES + 1).contains(&pivots) {
                return None;
            }
            let pieces = (0..pivots - 1)
                .map(|i| {
                    if curve.mapping_idc[i] == sys::MAPPING_MMR {
                        Piece::Mmr {
                            order: usize::from(curve.mmr_order[i]).clamp(1, 3),
                            constant: coef(curve.mmr_constant[i]),
                            coefficients: curve.mmr_coef[i].map(|power| power.map(coef)),
                        }
                    } else {
                        let order = usize::from(curve.poly_order[i]);
                        Piece::Polynomial(std::array::from_fn(|k| {
                            if k <= order {
                                coef(curve.poly_coef[i][k])
                            } else {
                                0.0
                            }
                        }))
                    }
                })
                .collect();
            curves.push(Curve {
                pivots: curve.pivots[..pivots]
                    .iter()
                    .map(|&p| (f64::from(p) * pivot_scale) as f32)
                    .collect(),
                pieces,
            });
        }

        let q = |r: ffi::AVRational| {
            if r.den == 0 {
                0.0
            } else {
                (f64::from(r.num) / f64::from(r.den)) as f32
            }
        };
        let matrix = |m: &[ffi::AVRational; 9]| -> [[f32; 3]; 3] {
            std::array::from_fn(|row| std::array::from_fn(|col| q(m[row * 3 + col])))
        };
        let nonlinear = matrix(&color.ycc_to_rgb_matrix);
        let offset = color.ycc_to_rgb_offset.map(q);
        let ipt_to_lms = nonlinear.map(|row| {
            let shift = row.iter().zip(offset).map(|(m, o)| m * o).sum::<f32>();
            [row[0], row[1], row[2], -shift]
        });
        let linear = matrix(&color.rgb_to_lms_matrix);
        let lms_to_rgb = std::array::from_fn(|row| {
            std::array::from_fn(|col| (0..3).map(|k| LMS_TO_BT2020[row][k] * linear[k][col]).sum())
        });
        let peak = hdr::pq_to_nits(f32::from(color.source_max_pq) / 4095.0);
        Some(Self {
            curves: curves.try_into().ok()?,
            ipt_to_lms,
            lms_to_rgb,
            peak_nits: hdr::plausible_peak(peak),
        })
    }

    /// The brightest its video gets, in nits, if it says: the peak of the
    /// display it was mastered on, which the reshaping leads to.
    pub fn peak_nits(&self) -> Option<f32> {
        self.peak_nits
    }

    /// PQ-encoded LMS from a pixel's three samples, each its code over
    /// 2^bit depth.
    pub fn lms_signal(&self, samples: [f32; 3]) -> [f32; 3] {
        let clamped: [f32; 3] = std::array::from_fn(|c| {
            samples[c].clamp(self.curves[c].first(), self.curves[c].last())
        });
        let ipt: [f32; 3] = std::array::from_fn(|c| self.curves[c].apply(clamped[c], clamped));
        self.ipt_to_lms.map(|row| {
            (row[0] * ipt[0] + row[1] * ipt[1] + row[2] * ipt[2] + row[3]).clamp(0.0, 1.0)
        })
    }

    /// Linear BT.2020 RGB from linear LMS (in any unit, which carries over).
    /// Colors outside BT.2020 are cut back to it.
    pub fn lms_to_rgb(&self, lms: [f32; 3]) -> [f32; 3] {
        self.lms_to_rgb
            .map(|row| (row[0] * lms[0] + row[1] * lms[1] + row[2] * lms[2]).max(0.0))
    }
}

#[cfg(test)]
#[path = "tests/dovi.rs"]
pub(crate) mod tests;
