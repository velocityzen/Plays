//! Dolby Vision without an HDR10 base layer (profile 5, as streaming
//! services use). Its pictures aren't YCbCr but IPT, the base layer of
//! Dolby's IPTPQc2, and only look right after the reshaping curves and
//! matrices that come with every frame in its RPU. FFmpeg parses those into
//! `AV_FRAME_DATA_DOVI_METADATA`. Shown as YCbCr instead, a picture turns
//! green and purple.
//!
//! Each of the three components goes through its own piecewise curve:
//! polynomials, or for chroma usually MMR, which mixes in the other two
//! components. Then a matrix turns IPT into PQ-encoded LMS. PQ's EOTF and a
//! second matrix lead to linear BT.2020 light, like any HDR10 picture, from
//! where `hdr` takes over. The math is libplacebo's (which mpv uses).
//!
//! gpui's Metal shader for Dolby Vision surfaces (`surface_dovi_fragment` in
//! vendor/gpui) does the same on the GPU, with the curves and matrices packed
//! by [`ShaderParams`]. This is the CPU version, for software decoding and
//! Linux.

use ffmpeg::ffi;
use ffmpeg::frame;
use ffmpeg_next as ffmpeg;

use super::hdr::ToneMapper;
use super::picture::bgra_rows;

/// FFmpeg's `AV_DOVI_MAX_PIECES`.
const MAX_PIECES: usize = 8;

/// From BT.2020's Hunt-Pointer-Estevez LMS, without crosstalk, to BT.2020
/// RGB. The RPU's own linear matrix comes first (libplacebo's values).
const LMS_TO_BT2020: [[f32; 3]; 3] = [
    [3.064_396_8, -2.165_961_3, 0.101_557_41],
    [-0.656_131_3, 1.785_569_4, -0.129_439_5],
    [0.017_360_633, -0.047_244_53, 1.029_889_5],
];

/// The parts of FFmpeg's structs in libavutil/dovi_meta.h that are read here,
/// which the bindings don't cover. Later FFmpeg versions only add fields at
/// the ends, and `AVDOVIMetadata` says where each struct starts.
#[allow(dead_code)]
mod sys {
    use super::MAX_PIECES;
    use ffmpeg_next::ffi::AVRational;

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

    #[repr(C)]
    pub struct DataMapping {
        pub vdr_rpu_id: u8,
        pub mapping_color_space: u8,
        pub mapping_chroma_format_idc: u8,
        pub curves: [ReshapingCurve; 3],
    }

    #[repr(C)]
    pub struct ColorMetadata {
        pub dm_metadata_id: u8,
        pub scene_refresh_flag: u8,
        pub ycc_to_rgb_matrix: [AVRational; 9],
        pub ycc_to_rgb_offset: [AVRational; 3],
        pub rgb_to_lms_matrix: [AVRational; 9],
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
pub struct Reshaping {
    curves: [Curve; 3],
    /// Rows from reshaped IPT to PQ-encoded LMS, with the RPU's offset
    /// folded into the last column.
    ipt_to_lms: [[f32; 4]; 3],
    /// Rows from linear LMS to linear BT.2020 RGB.
    lms_to_rgb: [[f32; 3]; 3],
}

impl Reshaping {
    /// The frame's reshaping if it needs one: it is Dolby Vision and its
    /// pictures aren't plain HDR (profile 5). Profile 8 and the like carry
    /// an HDR10 or HLG picture, tagged as such, which is shown as it is.
    pub fn of(frame: &frame::Video) -> Option<Self> {
        if super::hdr::Transfer::of(frame).is_some() {
            return None;
        }
        // SAFETY: side data stays valid as long as the frame; each struct is
        // checked to be within it before it is read.
        unsafe {
            let side = ffi::av_frame_get_side_data(
                frame.as_ptr(),
                ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_METADATA,
            );
            if side.is_null() {
                return None;
            }
            let (data, size) = ((*side).data as *const u8, (*side).size as usize);
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
            Self::new(
                &*(header as *const sys::RpuDataHeader),
                &*(mapping as *const sys::DataMapping),
                &*(color as *const sys::ColorMetadata),
            )
        }
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
                        let order = usize::from(curve.mmr_order[i]).clamp(1, 3);
                        Piece::Mmr {
                            order,
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
        Some(Self {
            curves: curves.try_into().ok()?,
            ipt_to_lms,
            lms_to_rgb,
        })
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

    /// Convert a `width` x `height` picture, scaled to 16-bit 4:4:4 planes
    /// by swscale (`planes` with their row strides in bytes), into tightly
    /// packed BGRA for an SDR screen. `mapper` is a PQ tone mapper.
    pub fn render(
        &self,
        mapper: &ToneMapper,
        planes: [(&[u8], usize); 3],
        width: usize,
        height: usize,
    ) -> Vec<u8> {
        bgra_rows(width, height, |y, out| {
            let rows = planes.map(|(data, stride)| &data[y * stride..][..width * 2]);
            let (out, _) = out.as_chunks_mut::<4>();
            for (x, out) in out.iter_mut().enumerate() {
                // 16-bit samples hold the base layer's codes in their top bits.
                let samples = rows.map(|row| {
                    f32::from(u16::from_le_bytes([row[2 * x], row[2 * x + 1]])) / 65536.0
                });
                let lms = self.lms_signal(samples).map(|s| mapper.pq_light(s));
                *out = mapper.output(self.lms_to_rgb(lms));
            }
        })
    }

    /// The reshaping and matrices as `surface_dovi_fragment` takes them.
    pub fn shader_params(&self) -> ShaderParams {
        let curves = self.curves.each_ref().map(|curve| {
            let mut pivots = [[2.0; 4]; 2];
            for (i, &pivot) in curve.pivots[1..curve.pivots.len() - 1].iter().enumerate() {
                pivots[i / 4][i % 4] = pivot;
            }
            let mut pieces = [ShaderPiece::default(); MAX_PIECES];
            for (piece, out) in curve.pieces.iter().zip(&mut pieces) {
                match *piece {
                    Piece::Polynomial([c0, c1, c2]) => out.poly = [c0, c1, c2, 0.0],
                    Piece::Mmr {
                        order,
                        constant,
                        coefficients,
                    } => {
                        out.poly = [constant, 0.0, 0.0, order as f32];
                        for (k, c) in coefficients.iter().enumerate() {
                            out.mmr[2 * k] = [c[0], c[1], c[2], 0.0];
                            out.mmr[2 * k + 1] = [c[3], c[4], c[5], c[6]];
                        }
                    }
                }
            }
            ShaderCurve {
                bounds: [curve.first(), curve.last(), curve.pieces.len() as f32, 0.0],
                pivots,
                pieces,
            }
        });
        ShaderParams {
            ipt_to_lms: self.ipt_to_lms,
            lms_to_rgb: self.lms_to_rgb.map(|[a, b, c]| [a, b, c, 0.0]),
            curves,
        }
    }
}

/// The reshaping and matrices for gpui's `surface_dovi_fragment`, which
/// gets these bytes as they are. Matches `DoviParams` in vendor/gpui's
/// shaders.metal: only `float4`s, so there is no padding to agree on.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShaderParams {
    /// Rows from reshaped IPT (and 1) to PQ-encoded LMS.
    ipt_to_lms: [[f32; 4]; 3],
    /// Rows from linear LMS to linear BT.2020 RGB.
    lms_to_rgb: [[f32; 4]; 3],
    curves: [ShaderCurve; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ShaderCurve {
    /// The first and last pivot, and the number of pieces.
    bounds: [f32; 4],
    /// The pivots between pieces, padded with 2 (above any signal).
    pivots: [[f32; 4]; 2],
    pieces: [ShaderPiece; MAX_PIECES],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ShaderPiece {
    /// A polynomial's coefficients of x⁰, x¹ and x², or MMR's constant;
    /// then MMR's order, 0 for a polynomial.
    poly: [f32; 4],
    /// MMR weights for each power: of s₀, s₁, s₂, then of s₀s₁, s₀s₂, s₁s₂,
    /// s₀s₁s₂.
    mmr: [[f32; 4]; 6],
}

impl ShaderParams {
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: the struct is plain f32s with no padding.
        unsafe { std::slice::from_raw_parts((self as *const Self).cast(), size_of::<Self>()) }
    }

    /// The params as a surface carries them.
    #[cfg(test)]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), size_of::<Self>());
        // SAFETY: the length matches, and any f32s are valid.
        unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast()) }
    }

    /// What `surface_dovi_fragment` computes from `samples`, before tone
    /// mapping: linear BT.2020 light in nits. For checking the packing.
    #[cfg(test)]
    pub fn light(&self, samples: [f32; 3]) -> [f32; 3] {
        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
        let sig: [f32; 3] = std::array::from_fn(|c| {
            let bounds = self.curves[c].bounds;
            samples[c].clamp(bounds[0], bounds[1])
        });
        let reshape = |curve: &ShaderCurve, s: f32| {
            let above = curve.pivots.iter().flatten().filter(|&&p| s >= p).count();
            let piece = &curve.pieces[above.min(curve.bounds[2] as usize - 1)];
            let order = piece.poly[3];
            if order == 0.0 {
                return (piece.poly[2] * s + piece.poly[1]) * s + piece.poly[0];
            }
            let [a, b, c] = sig;
            let base = [a, b, c];
            let cross = [a * b, a * c, b * c, a * b * c];
            let mut value = piece.poly[0];
            for k in 0..order as usize {
                let p = k as i32 + 1;
                value += dot(&piece.mmr[2 * k][..3], &base.map(|v| v.powi(p)));
                value += dot(&piece.mmr[2 * k + 1], &cross.map(|v| v.powi(p)));
            }
            value
        };
        let ipt: [f32; 3] =
            std::array::from_fn(|c| reshape(&self.curves[c], sig[c]).clamp(0.0, 1.0));
        let lms = self.ipt_to_lms.map(|row| {
            let signal = (dot(&row[..3], &ipt) + row[3]).clamp(0.0, 1.0);
            super::hdr::pq_to_nits(signal)
        });
        self.lms_to_rgb.map(|row| dot(&row[..3], &lms).max(0.0))
    }
}

#[cfg(test)]
mod tests {
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
        };
        (header, mapping, color)
    }

    fn reshaping() -> Reshaping {
        let (header, mapping, color) = profile5();
        Reshaping::new(&header, &mapping, &color).unwrap()
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
        assert_eq!(offset_of!(sys::ColorMetadata, ycc_to_rgb_offset), 76);
        assert_eq!(offset_of!(sys::ColorMetadata, rgb_to_lms_matrix), 100);
        // `DOVI_PARAMS_SIZE` in vendor/gpui's metal_renderer.rs, which is
        // within Metal's limit of 4 KB for bytes set straight on an encoder.
        assert_eq!(size_of::<ShaderParams>(), 2928);
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
}
