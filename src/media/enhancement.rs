//! Dolby Vision profile 7 (UHD Blu-ray): an HDR10 base layer, and an
//! enhancement layer that brings it back to the 12-bit master it was cut
//! from. Each frame's RPU maps the base layer's samples by curves (the
//! ones `dovi` reshapes profile 5 by) and dequantizes the enhancement
//! layer's into a residual added to them: the highlights past the base
//! layer's peak, and the detail and precision it lost. A minimal
//! enhancement layer (MEL) adds nothing; a full one (FEL) is what makes
//! profile 7 more than HDR10.
//!
//! The math is ETSI GS CCM 001's composer (clause 5.4), in its integer
//! arithmetic, with the enhancement layer brought to the base layer's size
//! by the filters of its annex B; but for MMR, which is in floating point
//! as libplacebo reshapes on the GPU, rows at a time: in integers it took
//! too long for 4K at 24 frames a second. MMR maps chroma by luma taken
//! over the six luma samples nearest each chroma sample, as the spec has
//! it; their weights didn't survive its PDF, and are taken as [1 2 1]
//! across and an even split down.
//!
//! What comes out is a plain HDR10 frame: BT.2020 PQ in limited range, as
//! the RPU's own matrix has it, at 10 bits as screens take it, with the
//! peak of the display it was mastered on. Whatever shows HDR10 shows it.

use anyhow::{Context as _, Result, bail, ensure};
use ffmpeg::ffi;
use ffmpeg::util::color;
use ffmpeg::util::format::Pixel;
use ffmpeg::{codec, frame};
use ffmpeg_next as ffmpeg;

use super::dovi::{self, sys};
use super::{hdr, picture};

/// Bits a composed sample has.
const OUT_DEPTH: u32 = 10;
/// Bits each layer's samples have: Blu-rays' both, and all the frames
/// read here (`Planes`) have.
const DEPTH: u32 = 10;

/// What a stream's Dolby Vision configuration record says it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configuration {
    pub profile: u8,
    pub level: u8,
    /// What its base layer shows as without Dolby Vision: 1 HDR10, 2 SDR,
    /// 4 HLG, 6 a Blu-ray's HDR10; 0 nothing else can show it.
    pub compatibility: u8,
    /// The base layer: the picture players without Dolby Vision show.
    pub base: bool,
    pub enhancement: bool,
}

impl Configuration {
    /// What a stream's parameters say, if it is Dolby Vision.
    pub fn of(parameters: &codec::Parameters) -> Option<Self> {
        // SAFETY: the parameters' coded side data stays valid while they
        // do. A DOVI_CONF entry is an `AVDOVIDecoderConfigurationRecord`,
        // checked to be as big as one; it is all bytes, so any address
        // will do.
        unsafe {
            let raw = parameters.as_ptr();
            let side = ffi::av_packet_side_data_get(
                (*raw).coded_side_data,
                (*raw).nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
            );
            if side.is_null()
                || (*side).data.is_null()
                || (*side).size < size_of::<sys::Configuration>()
            {
                return None;
            }
            let record = &*(*side).data.cast::<sys::Configuration>();
            Some(Self {
                profile: record.dv_profile,
                level: record.dv_level,
                compatibility: record.dv_bl_signal_compatibility_id,
                base: record.bl_present_flag != 0,
                enhancement: record.el_present_flag != 0,
            })
        }
    }

    /// Whether a stream is only an enhancement layer: a track of its own
    /// beside the base layer's, which never shows alone.
    pub fn is_layer_only(parameters: &codec::Parameters) -> bool {
        Self::of(parameters).is_some_and(|c| c.enhancement && !c.base)
    }
}

/// One piece of a component's mapping.
#[derive(Clone, Copy, Debug)]
enum Piece {
    /// Coefficients of x⁰, x¹ and x², up to `order`, in fixed point.
    Polynomial {
        order: usize,
        coefficients: [i64; 3],
    },
    Mmr(Mmr),
}

/// Multivariate multiple regression over all three components, each a
/// code over 2^bit depth, to the third power.
#[derive(Clone, Copy, Debug)]
struct Mmr {
    constant: f32,
    /// Per power: the weights of s₀, s₁, s₂, s₀s₁, s₀s₂, s₁s₂, s₀s₁s₂;
    /// none past its order.
    coefficients: [[f32; 7]; 3],
}

impl Mmr {
    /// The mapped value of components `[a, b, c]`: 16 bits.
    #[inline(always)]
    fn map(&self, [a, b, c]: [f32; 3]) -> u16 {
        let first = [a, b, c, a * b, a * c, b * c, a * b * c];
        let second = first.map(|t| t * t);
        let third: [f32; 7] = std::array::from_fn(|k| second[k] * first[k]);
        let dot = |w: &[f32; 7], t: &[f32; 7]| w.iter().zip(t).map(|(w, t)| w * t).sum::<f32>();
        let [w1, w2, w3] = &self.coefficients;
        let value = self.constant + dot(w1, &first) + dot(w2, &second) + dot(w3, &third);
        (value * 65536.0).clamp(0.0, 65535.0) as u16
    }
}

/// A component's mapping: pieces between pivots, which are base layer codes.
#[derive(Clone, Debug)]
struct Curve {
    pivots: Vec<i64>,
    pieces: Vec<Piece>,
}

impl Curve {
    /// The spec's pivot index (clause 5.4.2.2), of a code clamped to the
    /// pieces there are: the last one takes the last pivot itself.
    fn piece(&self, s: i64) -> &Piece {
        let inner = &self.pivots[1..self.pivots.len() - 1];
        let index = inner.iter().take_while(|&&pivot| s >= pivot).count();
        &self.pieces[index]
    }

    fn clamp(&self, s: i64) -> i64 {
        s.clamp(self.pivots[0], self.pivots[self.pivots.len() - 1])
    }

    fn is_polynomial(&self) -> bool {
        self.pieces
            .iter()
            .all(|piece| matches!(piece, Piece::Polynomial { .. }))
    }
}

/// How a component's enhancement layer is dequantized, in fixed point.
#[derive(Clone, Copy, Debug)]
struct Nlq {
    offset: i64,
    in_max: i64,
    slope: i64,
    threshold: i64,
}

/// A frame's composing, from its RPU.
#[derive(Clone, Debug)]
pub struct Composer {
    bl_depth: u32,
    el_depth: u32,
    /// Bits of fraction the coefficients have.
    denom: u32,
    curves: [Curve; 3],
    /// Each curve's first and last pivot, which MMR clamps each code to.
    ranges: [(i64, i64); 3],
    /// Each component's mapping of every base layer code, for those that
    /// are polynomials all through: luma always, chroma usually not.
    mapped: [Option<Vec<u16>>; 3],
    nlq: [Nlq; 3],
    /// Each component's residual for every enhancement layer code.
    residuals: [Vec<i32>; 3],
    peak_nits: Option<f32>,
}

impl Composer {
    /// The composing a frame's RPU says, if it has an enhancement layer
    /// this can compose: one residual over the whole picture, dequantized
    /// linearly with a dead zone, onto Y'CbCr.
    pub fn of(frame: &frame::Video) -> Option<Self> {
        let (header, mapping, color) = dovi::rpu(frame)?;
        Self::new(header, mapping, color)
    }

    fn new(
        header: &sys::RpuDataHeader,
        mapping: &sys::DataMapping,
        color: &sys::ColorMetadata,
    ) -> Option<Self> {
        let (bl_depth, el_depth, denom) = (
            u32::from(header.bl_bit_depth),
            u32::from(header.el_bit_depth),
            u32::from(header.coef_log2_denom),
        );
        if header.disable_residual_flag != 0
            || mapping.mapping_color_space != 0
            || mapping.nlq_method_idc != sys::NLQ_LINEAR_DZ
            || (mapping.num_x_partitions, mapping.num_y_partitions) != (1, 1)
            || (bl_depth, el_depth) != (DEPTH, DEPTH)
            || denom > 32
        {
            return None;
        }
        let curves: [Curve; 3] = std::array::from_fn(|c| curve(&mapping.curves[c], denom));
        if curves.iter().any(|curve| curve.pieces.is_empty()) || !curves[0].is_polynomial() {
            return None;
        }
        let mapped = std::array::from_fn(|c| {
            let curve = &curves[c];
            curve.is_polynomial().then(|| {
                (0..1i64 << bl_depth)
                    .map(|s| match *curve.piece(s) {
                        Piece::Polynomial {
                            order,
                            coefficients,
                        } => polynomial(curve.clamp(s), order, &coefficients, bl_depth, denom),
                        Piece::Mmr(_) => unreachable!("the curve is polynomial"),
                    })
                    .collect()
            })
        });
        // Past any coefficient FFmpeg would parse, and far from overflowing.
        let fixed = |v: u64| i64::try_from(v).unwrap_or(i64::MAX).min(1 << 40);
        let nlq = mapping.nlq.map(|p| Nlq {
            offset: i64::from(p.nlq_offset),
            in_max: fixed(p.vdr_in_max),
            slope: fixed(p.linear_deadzone_slope),
            threshold: fixed(p.linear_deadzone_threshold),
        });
        let residuals = nlq.map(|nlq| {
            (0..1i64 << el_depth)
                .map(|e| dequantize(e, nlq, el_depth, denom))
                .collect()
        });
        let peak = hdr::pq_to_nits(f32::from(color.source_max_pq) / 4095.0);
        let ranges = curves
            .each_ref()
            .map(|curve| (curve.pivots[0], curve.pivots[curve.pivots.len() - 1]));
        Some(Self {
            bl_depth,
            el_depth,
            denom,
            curves,
            ranges,
            mapped,
            nlq,
            residuals,
            peak_nits: hdr::plausible_peak(peak),
        })
    }

    /// Compose the base and enhancement layers of one picture into `out`,
    /// made anew unless it is a frame this made before, which isn't shared.
    /// The enhancement layer is as big as the base layer, or half as wide
    /// and as high.
    pub fn compose(
        &self,
        base: &frame::Video,
        enhancement: &frame::Video,
        out: &mut frame::Video,
    ) -> Result<()> {
        let bl = Planes::of(base).context("the base layer isn't 10-bit 4:2:0, top down")?;
        let el = Planes::of(enhancement)
            .context("the enhancement layer isn't 10-bit 4:2:0, top down")?;
        let size = (bl.0[0].width, bl.0[0].height);
        let upscale = match (el.0[0].width, el.0[0].height) {
            el_size if el_size == size => false,
            (width, height) if (2 * width, 2 * height) == size => true,
            (width, height) => bail!(
                "a {width}x{height} enhancement layer can't go with a {}x{} base layer",
                size.0,
                size.1
            ),
        };
        let (width, height) = (base.width(), base.height());
        // SAFETY: reading a flag of a valid frame.
        let writable = unsafe { ffi::av_frame_is_writable(out.as_mut_ptr()) != 0 };
        if out.format() != Pixel::YUV420P10LE
            || (out.width(), out.height()) != (width, height)
            || !writable
        {
            *out = frame::Video::new(Pixel::YUV420P10LE, width, height);
        }
        for c in 0..3 {
            let width = bl.0[c].width;
            let layer = Codes::of(&el.0[c]);
            let (plane, stride) =
                ffmpeg_safe::plane_mut(out, c).context("the frame made for it has no plane")?;
            picture::in_rows(plane, stride, |y, row| {
                let el_row = enhancement_row(&layer, c > 0, y, width, upscale);
                let mapped = self.mapped_row(&bl, c, y);
                let (row, _) = row[..width * 2].as_chunks_mut::<2>();
                for ((out, &v), &e) in row.iter_mut().zip(&mapped).zip(&el_row) {
                    *out = reconstruct(v, self.residual(c, e)).to_le_bytes();
                }
            });
        }
        self.tag(base, out)
    }

    /// Row `y` of component `c`'s mapped base layer: 16-bit values.
    fn mapped_row(&self, bl: &Planes, c: usize, y: usize) -> Vec<u16> {
        let own = bl.0[c].row(y);
        if let Some(mapped) = &self.mapped[c] {
            return own
                .iter()
                .map(|&s| mapped[usize::from(s).min(mapped.len() - 1)])
                .collect();
        }
        // MMR, over chroma: luma comes down to chroma's size.
        let other = bl.0[3 - c].row(y);
        let luma = &bl.0[0];
        let (top, bottom) = (luma.row(2 * y), luma.row((2 * y + 1).min(luma.height - 1)));
        let curve = &self.curves[c];
        let samples = own
            .iter()
            .zip(&other)
            .enumerate()
            .map(|(x, (&own, &other))| {
                let s0 = downsampled_luma(&top, &bottom, x);
                let (s1, s2) = if c == 1 { (own, other) } else { (other, own) };
                [s0, i64::from(s1), i64::from(s2)]
            });
        // Usually one piece, of MMR, all along: a row at a time, which
        // vectorizes.
        let [Piece::Mmr(mmr)] = &curve.pieces[..] else {
            return samples.map(|samples| self.map(curve, samples, c)).collect();
        };
        let scaled: Vec<[f32; 3]> = samples.map(|samples| self.scaled(samples)).collect();
        scaled.iter().map(|&samples| mmr.map(samples)).collect()
    }

    /// MMR's samples: each code clamped to its curve, over 2^bit depth.
    fn scaled(&self, samples: [i64; 3]) -> [f32; 3] {
        let scale = 1.0 / (1u32 << self.bl_depth) as f32;
        std::array::from_fn(|k| {
            let (low, high) = self.ranges[k];
            samples[k].clamp(low, high) as f32 * scale
        })
    }

    /// Chroma component `c`'s sample mapped by its curve's piece, MMR or
    /// not, from all three components' `samples`.
    fn map(&self, curve: &Curve, samples: [i64; 3], c: usize) -> u16 {
        let own = samples[c];
        let piece = match &curve.pieces[..] {
            [only] => only,
            _ => curve.piece(own),
        };
        match piece {
            Piece::Polynomial {
                order,
                coefficients,
            } => polynomial(
                curve.clamp(own),
                *order,
                coefficients,
                self.bl_depth,
                self.denom,
            ),
            Piece::Mmr(mmr) => mmr.map(self.scaled(samples)),
        }
    }

    /// Component `c`'s residual for an enhancement layer code, which an
    /// upscaled one can be past the largest of.
    fn residual(&self, c: usize, e: u16) -> i32 {
        self.residuals[c]
            .get(usize::from(e))
            .copied()
            .unwrap_or_else(|| dequantize(i64::from(e), self.nlq[c], self.el_depth, self.denom))
    }

    /// Give the composed frame the base layer's timing and an HDR10
    /// frame's tags and peak, without the RPU, which it has been composed
    /// by.
    fn tag(&self, base: &frame::Video, out: &mut frame::Video) -> Result<()> {
        // SAFETY: both frames are valid; the composed one's side data is
        // freed before the base layer's is copied over, so none is kept
        // from a frame before.
        unsafe {
            let raw = out.as_mut_ptr();
            ffi::av_frame_side_data_free(&mut (*raw).side_data, &mut (*raw).nb_side_data);
            ensure!(
                ffi::av_frame_copy_props(raw, base.as_ptr()) >= 0,
                "no memory for a composed frame's side data"
            );
            for kind in [
                ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_METADATA,
                ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_RPU_BUFFER,
            ] {
                ffi::av_frame_remove_side_data(raw, kind);
            }
        }
        out.set_color_space(color::Space::BT2020NCL);
        out.set_color_primaries(color::Primaries::BT2020);
        out.set_color_transfer_characteristic(color::TransferCharacteristic::SMPTE2084);
        out.set_color_range(color::Range::MPEG);
        if let Some(nits) = self.peak_nits {
            // The base layer's HDR10 metadata tells of the base layer's
            // peak; the composed picture's is the master's.
            // SAFETY: as above.
            unsafe {
                for kind in [
                    ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL,
                    ffi::AVFrameSideDataType::AV_FRAME_DATA_MASTERING_DISPLAY_METADATA,
                ] {
                    ffi::av_frame_remove_side_data(out.as_mut_ptr(), kind);
                }
            }
            hdr::attach_peak(out, nits)?;
        }
        Ok(())
    }
}

/// A component's mapping from FFmpeg's, whose coefficients have `denom`
/// bits of fraction, with no pieces if it makes no sense.
fn curve(curve: &sys::ReshapingCurve, denom: u32) -> Curve {
    let pivots = usize::from(curve.num_pivots);
    if !(2..=curve.pivots.len()).contains(&pivots) {
        return Curve {
            pivots: vec![0, 0],
            pieces: Vec::new(),
        };
    }
    let pieces = (0..pivots - 1)
        .map(|i| {
            if curve.mapping_idc[i] == sys::MAPPING_MMR {
                let real = |c: i64| (c as f64 / (1u64 << denom) as f64) as f32;
                let order = usize::from(curve.mmr_order[i]);
                Piece::Mmr(Mmr {
                    constant: real(curve.mmr_constant[i]),
                    coefficients: std::array::from_fn(|k| {
                        if k < order {
                            curve.mmr_coef[i][k].map(real)
                        } else {
                            [0.0; 7]
                        }
                    }),
                })
            } else {
                Piece::Polynomial {
                    order: usize::from(curve.poly_order[i]).min(2),
                    coefficients: curve.poly_coef[i],
                }
            }
        })
        .collect();
    Curve {
        pivots: curve.pivots[..pivots]
            .iter()
            .map(|&p| i64::from(p))
            .collect(),
        pieces,
    }
}

/// A base layer code `s`, clamped to its curve, mapped by a polynomial
/// (clause 5.4.2.3.2): 16 bits.
fn polynomial(s: i64, order: usize, coefficients: &[i64; 3], bl_depth: u32, denom: u32) -> u16 {
    // Wider than the spec's 64 bits, for coefficients no encoder makes.
    let (mut power, mut shift, mut sum) = (1i128, 20i32, 0i128);
    for &c in &coefficients[..=order] {
        sum += i128::from(c) * (power << shift);
        power *= i128::from(s);
        shift -= bl_depth as i32;
    }
    (sum.max(0) >> (4 + denom)).min(0xffff) as u16
}

/// An enhancement layer code dequantized: linear, with a dead zone around
/// the offset (clause 5.4.3.2), in the mapped base layer's 16-bit units.
fn dequantize(e: i64, nlq: Nlq, el_depth: u32, denom: u32) -> i32 {
    let difference = e - nlq.offset;
    if difference == 0 {
        return 0;
    }
    let sign = difference.signum();
    let spread = ((difference << 1) - sign) << (10 - el_depth);
    let limit = nlq.in_max << (11 - el_depth);
    let dq = (spread * nlq.slope + (nlq.threshold << (11 - el_depth)) * sign).clamp(-limit, limit);
    let shift = denom as i32 - 5 - el_depth as i32;
    let r = if shift >= 0 {
        dq >> shift
    } else {
        dq << -shift
    };
    r.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The mapped base layer and its residual made one sample of the master
/// (clause 5.4.3.3), rounded to `OUT_DEPTH` bits.
fn reconstruct(mapped: u16, residual: i32) -> u16 {
    let h = i64::from(mapped) + i64::from(residual);
    let h = (h + (1 << (15 - OUT_DEPTH))) >> (16 - OUT_DEPTH);
    h.clamp(0, (1 << OUT_DEPTH) - 1) as u16
}

/// Luma at chroma sample `x` of the rows of luma `top` and `bottom` it
/// lies between: the three across around it, [1 2 1], in both rows.
fn downsampled_luma(top: &[u16], bottom: &[u16], x: usize) -> i64 {
    let last = top.len() - 1;
    let (left, middle, right) = (
        (2 * x).saturating_sub(1).min(last),
        (2 * x).min(last),
        (2 * x + 1).min(last),
    );
    let column =
        |row: &[u16]| i64::from(row[left]) + 2 * i64::from(row[middle]) + i64::from(row[right]);
    (column(top) + column(bottom) + 4) >> 3
}

/// The vertical filters of annex B: per output row parity, the taps as
/// offsets from the input row and weights, and the bits they sum to.
const LUMA_DOWN: [[(isize, i32); 4]; 2] = [
    [(-2, -3), (-1, 29), (0, 111), (1, -9)],
    [(-1, -9), (0, 111), (1, 29), (2, -3)],
];
const LUMA_DOWN_BITS: u32 = 7;
const CHROMA_DOWN: [[(isize, i32); 2]; 2] = [[(-1, 64), (0, 192)], [(0, 192), (1, 64)]];
const CHROMA_DOWN_BITS: u32 = 8;
/// Annex B's horizontal filter, for the sample between input samples n and
/// n + 1, over n - 3 to n + 4; the even ones are the input's own.
const ACROSS: [i32; 8] = [22, 94, -524, 2456, 2456, -524, 94, 22];
const ACROSS_BITS: u32 = 12;

/// Row `y` of an enhancement layer component at the base layer's size,
/// `width` codes: as it is, or doubled each way by annex B's filters,
/// down first, then across. Samples past an edge are the edge's.
fn enhancement_row(plane: &Codes, chroma: bool, y: usize, width: usize, upscale: bool) -> Vec<u16> {
    if !upscale {
        let mut row = plane.row(y.min(plane.height - 1)).to_vec();
        row.resize(width, 0);
        return row;
    }
    let source = |offset: isize| {
        let row = (y / 2) as isize + offset;
        plane.row(row.clamp(0, plane.height as isize - 1) as usize)
    };
    let parity = y % 2;
    let (taps, bits): (&[(isize, i32)], u32) = if chroma {
        (&CHROMA_DOWN[parity], CHROMA_DOWN_BITS)
    } else {
        (&LUMA_DOWN[parity], LUMA_DOWN_BITS)
    };
    // Down, with the samples past each end the end's, for the filter
    // across.
    let (before, after) = (3, 4);
    let mut down = vec![0i32; before + plane.width + after];
    for &(offset, weight) in taps {
        for (sum, &sample) in down[before..].iter_mut().zip(source(offset)) {
            *sum += weight * i32::from(sample);
        }
    }
    for sum in &mut down[before..before + plane.width] {
        *sum = i32::from(clip((*sum + (1 << (bits - 1))) >> bits));
    }
    let (first, last) = (down[before], down[before + plane.width - 1]);
    down[..before].fill(first);
    down[before + plane.width..].fill(last);
    let mut row = vec![0u16; width];
    for (pair, window) in row.chunks_mut(2).zip(down.windows(ACROSS.len())) {
        // The sample itself, then the one between it and the next.
        pair[0] = window[before] as u16;
        if let Some(between) = pair.get_mut(1) {
            let sum: i32 = ACROSS.iter().zip(window).map(|(w, s)| w * s).sum();
            *between = clip((sum + (1 << (ACROSS_BITS - 1))) >> ACROSS_BITS);
        }
    }
    row
}

fn clip(value: i32) -> u16 {
    value.clamp(0, 0xffff) as u16
}

/// One component's samples in a frame's memory.
#[derive(Clone, Copy)]
struct Plane<'a> {
    data: &'a [u8],
    stride: usize,
    width: usize,
    height: usize,
    /// Samples from one of this component's to the next: 2 where chroma
    /// is interleaved.
    step: usize,
    /// Where its first is, in samples.
    first: usize,
    /// Bits the code is above the sample's bottom: P010's are its top 10.
    shift: u32,
}

impl Plane<'_> {
    /// Row `y`'s codes.
    fn row(&self, y: usize) -> Vec<u16> {
        let row = &self.data[y * self.stride..];
        (0..self.width)
            .map(|x| {
                let i = 2 * (x * self.step + self.first);
                u16::from_le_bytes([row[i], row[i + 1]]) >> self.shift
            })
            .collect()
    }
}

/// One component's codes, all of them.
struct Codes {
    codes: Vec<u16>,
    width: usize,
    height: usize,
}

impl Codes {
    fn of(plane: &Plane) -> Self {
        Self {
            codes: picture::rows(plane.width, plane.height, |y, out| {
                out.copy_from_slice(&plane.row(y));
            }),
            width: plane.width,
            height: plane.height,
        }
    }

    fn row(&self, y: usize) -> &[u16] {
        &self.codes[y * self.width..][..self.width]
    }
}

/// A frame's three components, if it is 10-bit 4:2:0 in memory, stored
/// top down.
struct Planes<'a>([Plane<'a>; 3]);

impl<'a> Planes<'a> {
    fn of(frame: &'a frame::Video) -> Option<Self> {
        let (width, height) = (frame.width() as usize, frame.height() as usize);
        let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
        let plane = |index: usize, width, height, step, first, shift| {
            let (data, stride) = ffmpeg_safe::plane(frame, index)?;
            Some(Plane {
                data,
                stride,
                width,
                height,
                step,
                first,
                shift,
            })
        };
        let planes_in = match frame.format() {
            Pixel::YUV420P10LE => 3,
            Pixel::P010LE => 2,
            _ => return None,
        };
        if width == 0 || height == 0 || frame.planes() < planes_in {
            return None;
        }
        let planes = match frame.format() {
            Pixel::YUV420P10LE => [
                plane(0, width, height, 1, 0, 0)?,
                plane(1, chroma_width, chroma_height, 1, 0, 0)?,
                plane(2, chroma_width, chroma_height, 1, 0, 0)?,
            ],
            Pixel::P010LE => [
                plane(0, width, height, 1, 0, 6)?,
                plane(1, chroma_width, chroma_height, 2, 0, 6)?,
                plane(1, chroma_width, chroma_height, 2, 1, 6)?,
            ],
            _ => return None,
        };
        Some(Self(planes))
    }
}

#[cfg(test)]
#[path = "tests/enhancement.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "tests/enhancement_properties.rs"]
mod properties;
