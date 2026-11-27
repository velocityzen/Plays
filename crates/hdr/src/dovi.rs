//! Dolby Vision whose pictures are IPT, not YCbCr (profiles 5, 10.0 and 20):
//! they only look right reshaped by the curves and matrices each frame's
//! RPU carries, which FFmpeg parses into `AV_FRAME_DATA_DOVI_METADATA`.
//! Shown as YCbCr instead, a picture turns green and purple.
//!
//! Each component goes through its own piecewise curve: polynomials, or for
//! chroma usually MMR, which mixes in the other two. Then a matrix turns
//! IPT into PQ-encoded LMS, and PQ's EOTF and a second matrix lead to
//! linear BT.2020 light, like any HDR10 picture's. The math is libplacebo's
//! (which mpv uses). gpui's `surface_dovi_fragment` does the same on the
//! GPU, with the curves and matrices packed by [`ShaderParams`].

use ffmpeg_next::{ffi, frame};
use ffmpeg_safe::Rpu;
use ffmpeg_safe::sys::{self, MAX_PIECES};

use crate::{plausible_peak, pq_to_nits};

/// From BT.2020's Hunt-Pointer-Estevez LMS, without crosstalk, to BT.2020
/// RGB. The RPU's own linear matrix comes first (libplacebo's values).
const LMS_TO_BT2020: [[f32; 3]; 3] = [
    [3.064_396_8, -2.165_961_3, 0.101_557_41],
    [-0.656_131_3, 1.785_569_4, -0.129_439_5],
    [0.017_360_633, -0.047_244_53, 1.029_889_5],
];

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
    /// The peak of the display it was mastered on, in nits, if it says.
    peak_nits: Option<f32>,
}

impl Reshaping {
    /// The reshaping `frame`'s RPU gives, if it has one that can be read,
    /// however the frame is tagged.
    pub fn of(frame: &frame::Video) -> Option<Self> {
        let Rpu {
            header,
            mapping,
            color,
        } = ffmpeg_safe::rpu(frame)?;
        Self::new(header, mapping, color)
    }

    pub(crate) fn new(
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
        Some(Self {
            curves: curves.try_into().ok()?,
            ipt_to_lms,
            lms_to_rgb,
            peak_nits: mastered_peak(color),
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

/// The peak of the display a Dolby Vision video was mastered on, in nits,
/// as its RPU's `color` says, if a video could be mastered at it.
pub fn mastered_peak(color: &sys::ColorMetadata) -> Option<f32> {
    plausible_peak(pq_to_nits(f32::from(color.source_max_pq) / 4095.0))
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

    /// The params as a surface carries them, if they're as long.
    #[cfg(any(test, feature = "testing"))]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != size_of::<Self>() {
            return None;
        }
        // SAFETY: the length matches, and any f32s are valid.
        Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast()) })
    }

    /// What `surface_dovi_fragment` computes from `samples`, before tone
    /// mapping: linear BT.2020 light in nits. For checking the packing.
    #[cfg(any(test, feature = "testing"))]
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
            pq_to_nits(signal)
        });
        self.lms_to_rgb.map(|row| dot(&row[..3], &lms).max(0.0))
    }
}

#[cfg(test)]
#[path = "tests/dovi.rs"]
mod tests;
