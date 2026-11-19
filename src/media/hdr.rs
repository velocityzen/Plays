//! HDR video: PQ (HDR10) and HLG pictures in BT.2020, and fitting them to
//! what a screen can show.
//!
//! Brightness is measured against SDR white, the white of everything else on
//! screen: HDR's reference white of 203 nits (ITU-R BT.2408) lands there, and
//! highlights go above it as far as the screen allows. That limit is the
//! screen's headroom: on macOS its current EDR headroom, on an SDR screen 1.
//! The tone curve leaves everything below three quarters of the headroom alone
//! and rolls the rest off smoothly toward it, so nothing clips.
//!
//! gpui's Metal shader for HDR surfaces (`surface_hdr_fragment` in
//! vendor/gpui) does the same on the GPU, with these same constants. This is
//! the CPU version, for software decoding and Linux, always for an SDR screen.
//! Dolby Vision's profile 5 needs more before it is HDR like the rest
//! (`dovi`).

use ffmpeg::frame;
use ffmpeg::util::color::TransferCharacteristic;
use ffmpeg_next as ffmpeg;

use super::picture::{apply_gamma, bgra_rows};

/// HDR reference white (BT.2408), shown as SDR white.
pub const REFERENCE_WHITE_NITS: f32 = 203.0;
/// The display HLG is rendered for (BT.2100's nominal 1000 nits), which sets
/// its system gamma of 1.2.
const HLG_PEAK_NITS: f32 = 1000.0;
const HLG_GAMMA: f32 = 1.2;
/// Brightness below this share of the headroom is left alone.
const KNEE: f32 = 0.75;

const BT2020_TO_BT709: [[f32; 3]; 3] = [
    [1.660_491, -0.587_641, -0.072_850],
    [-0.124_550, 1.1329, -0.008_349],
    [-0.018_151, -0.100_579, 1.118_73],
];
const BT709_TO_BT2020: [[f32; 3]; 3] = [
    [0.627_404, 0.329_283, 0.043_313],
    [0.069_097, 0.919_540, 0.011_362],
    [0.016_391, 0.088_013, 0.895_595],
];
/// HLG's OETF constants (BT.2100).
const HLG_A: f32 = 0.178_832_77;
const HLG_B: f32 = 1.0 - 4.0 * HLG_A;
const HLG_C: f32 = 0.559_910_7;

/// How an HDR picture encodes light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    /// SMPTE ST 2084, used by HDR10, HDR10+ and most Dolby Vision.
    Pq,
    /// Hybrid log-gamma (ARIB STD-B67), used by broadcasters.
    Hlg,
}

impl Transfer {
    /// The frame's HDR transfer function, or `None` for SDR.
    pub fn of(frame: &frame::Video) -> Option<Self> {
        match frame.color_transfer_characteristic() {
            TransferCharacteristic::SMPTE2084 => Some(Self::Pq),
            TransferCharacteristic::ARIB_STD_B67 => Some(Self::Hlg),
            _ => None,
        }
    }
}

/// PQ's constants (SMPTE ST 2084).
const PQ_M1: f32 = 2610.0 / 16384.0;
const PQ_M2: f32 = 2523.0 / 4096.0 * 128.0;
const PQ_C1: f32 = 3424.0 / 4096.0;
const PQ_C2: f32 = 2413.0 / 4096.0 * 32.0;
const PQ_C3: f32 = 2392.0 / 4096.0 * 32.0;

/// PQ's EOTF: a signal in 0..=1 to nits.
pub fn pq_to_nits(signal: f32) -> f32 {
    let p = signal.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    10000.0 * ((p - PQ_C1).max(0.0) / (PQ_C2 - PQ_C3 * p)).powf(1.0 / PQ_M1)
}

/// PQ's inverse EOTF: nits to a signal in 0..=1.
pub fn nits_to_pq(nits: f32) -> f32 {
    let y = (nits / 10000.0).clamp(0.0, 1.0).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * y) / (1.0 + PQ_C3 * y)).powf(PQ_M2)
}

/// HLG's inverse OETF: a signal in 0..=1 to scene light in 0..=1.
pub fn hlg_to_scene(signal: f32) -> f32 {
    let e = signal.clamp(0.0, 1.0);
    if e <= 0.5 {
        e * e / 3.0
    } else {
        (((e - HLG_C) / HLG_A).exp() + HLG_B) / 12.0
    }
}

/// HLG's OETF: scene light in 0..=1 to a signal in 0..=1.
pub fn scene_to_hlg(light: f32) -> f32 {
    let e = light.clamp(0.0, 1.0);
    if e <= 1.0 / 12.0 {
        (3.0 * e).sqrt()
    } else {
        HLG_A * (12.0 * e - HLG_B).ln() + HLG_C
    }
}

fn bt2020_luma(rgb: [f32; 3]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}

/// HLG's OOTF: scene light in BT.2020 RGB to nits on its nominal display.
pub fn hlg_scene_to_nits(rgb: [f32; 3]) -> [f32; 3] {
    let gain = HLG_PEAK_NITS * bt2020_luma(rgb).max(0.0).powf(HLG_GAMMA - 1.0);
    rgb.map(|c| gain * c)
}

/// HLG's inverse OOTF: nits on its nominal display, in BT.2020 RGB, to
/// scene light.
pub fn hlg_nits_to_scene(rgb: [f32; 3]) -> [f32; 3] {
    let luma = bt2020_luma(rgb) / HLG_PEAK_NITS;
    if luma <= 0.0 {
        return [0.0; 3];
    }
    let gain = luma.powf((1.0 - HLG_GAMMA) / HLG_GAMMA) / HLG_PEAK_NITS;
    rgb.map(|c| gain * c)
}

/// Linear BT.2020 RGB to linear BT.709 (sRGB) RGB.
pub fn bt2020_to_bt709(rgb: [f32; 3]) -> [f32; 3] {
    BT2020_TO_BT709.map(|row| row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2])
}

/// Linear BT.709 (sRGB) RGB to linear BT.2020 RGB.
pub fn bt709_to_bt2020(rgb: [f32; 3]) -> [f32; 3] {
    BT709_TO_BT2020.map(|row| row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2])
}

/// Linear BT.2020 light, relative to SDR white, as an HDR signal in 0..=1.
pub fn encode(light: [f32; 3], transfer: Transfer) -> [f32; 3] {
    let nits = light.map(|c| c.max(0.0) * REFERENCE_WHITE_NITS);
    match transfer {
        Transfer::Pq => nits.map(nits_to_pq),
        Transfer::Hlg => hlg_nits_to_scene(nits).map(scene_to_hlg),
    }
}

/// An sRGB color as an HDR signal, its white at HDR reference white: what
/// SDR things (subtitles) drawn over HDR video should be.
pub fn sdr_to_hdr(srgb: [f32; 3], transfer: Transfer) -> [f32; 3] {
    encode(bt709_to_bt2020(srgb.map(srgb_decode)), transfer)
}

/// Brightness relative to SDR white, fitted under `headroom`: unchanged up
/// to the knee, then easing toward the headroom without reaching it. The
/// slope is 1 on both sides of the knee, so there is no visible seam.
pub fn tone_curve(x: f32, headroom: f32) -> f32 {
    let knee = KNEE * headroom;
    if x <= knee {
        return x;
    }
    let room = headroom - knee;
    knee + room * (1.0 - (-(x - knee) / room).exp())
}

/// Fit a color's brightness under `headroom` without shifting its hue: the
/// brightest channel follows the tone curve and the others scale with it.
pub fn tone_map(rgb: [f32; 3], headroom: f32) -> [f32; 3] {
    let peak = rgb[0].max(rgb[1]).max(rgb[2]);
    if peak <= 0.0 {
        return rgb;
    }
    let scale = tone_curve(peak, headroom) / peak;
    rgb.map(|c| c * scale)
}

/// sRGB's encoding of linear light in 0..=1.
pub fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB's decoding to linear light in 0..=1.
pub fn srgb_decode(signal: f32) -> f32 {
    if signal <= 0.040_45 {
        signal / 12.92
    } else {
        ((signal + 0.055) / 1.055).powf(2.4)
    }
}

/// Entries in the decoding table: the top 12 bits of each 16-bit sample,
/// more than the 10 bits HDR video carries.
const DECODE_BITS: u32 = 12;
/// Steps in the encoding table, enough that dark tones don't band.
const ENCODE_STEPS: usize = 16384;

/// Turns HDR pictures, as nonlinear BT.2020 RGB48 from swscale, into 8-bit
/// BGRA for an SDR screen.
pub struct ToneMapper {
    transfer: Transfer,
    /// Signal to linear light: relative to SDR white for PQ, scene light
    /// for HLG (whose OOTF needs all three channels).
    decode: Vec<f32>,
    /// Linear light in 0..=1 to an sRGB byte, with `gamma` applied.
    encode: Vec<u8>,
    gamma: f32,
}

impl ToneMapper {
    pub fn new(transfer: Transfer) -> Self {
        let max = ((1 << DECODE_BITS) - 1) as f32;
        let decode = (0..1 << DECODE_BITS)
            .map(|i| {
                let signal = i as f32 / max;
                match transfer {
                    Transfer::Pq => pq_to_nits(signal) / REFERENCE_WHITE_NITS,
                    Transfer::Hlg => hlg_to_scene(signal),
                }
            })
            .collect();
        Self {
            transfer,
            decode,
            encode: Self::encode_table(1.0),
            gamma: 1.0,
        }
    }

    fn encode_table(gamma: f32) -> Vec<u8> {
        (0..=ENCODE_STEPS)
            .map(|i| {
                let signal = srgb_encode(i as f32 / ENCODE_STEPS as f32);
                (apply_gamma(signal, gamma) * 255.0).round() as u8
            })
            .collect()
    }

    /// Apply `gamma` (see `picture::apply_gamma`) to pictures from now on.
    /// For an SDR screen it comes after tone mapping, as on the GPU.
    pub fn set_gamma(&mut self, gamma: f32) {
        if gamma != self.gamma {
            self.encode = Self::encode_table(gamma);
            self.gamma = gamma;
        }
    }

    /// One pixel, from 16-bit nonlinear BT.2020 RGB to BGRA.
    fn pixel(&self, rgb: [u16; 3]) -> [u8; 4] {
        let light = rgb.map(|c| self.decode[(c >> (16 - DECODE_BITS)) as usize]);
        let relative = match self.transfer {
            Transfer::Pq => light,
            Transfer::Hlg => hlg_scene_to_nits(light).map(|nits| nits / REFERENCE_WHITE_NITS),
        };
        self.output(relative)
    }

    /// Linear light relative to SDR white, for a PQ signal in 0..=1; read
    /// between the table's entries. Only for a PQ tone mapper.
    pub fn pq_light(&self, signal: f32) -> f32 {
        debug_assert_eq!(self.transfer, Transfer::Pq);
        let position = signal.clamp(0.0, 1.0) * ((1 << DECODE_BITS) - 1) as f32;
        let index = position as usize;
        let below = self.decode[index];
        let above = self.decode[(index + 1).min(self.decode.len() - 1)];
        below + (above - below) * (position - index as f32)
    }

    /// Linear BT.2020 light, relative to SDR white, to a BGRA pixel.
    pub fn output(&self, light: [f32; 3]) -> [u8; 4] {
        let [r, g, b] = tone_map(bt2020_to_bt709(light), 1.0);
        let encode = |c: f32| self.encode[(c.clamp(0.0, 1.0) * ENCODE_STEPS as f32 + 0.5) as usize];
        [encode(b), encode(g), encode(r), 255]
    }

    /// Convert a `width` x `height` RGB48LE picture, `stride` bytes a row,
    /// into tightly packed BGRA.
    pub fn run(&self, data: &[u8], stride: usize, width: usize, height: usize) -> Vec<u8> {
        bgra_rows(width, height, |y, out| {
            let row = &data[y * stride..][..width * 6];
            let (pixels, _) = row.as_chunks::<6>();
            let (outs, _) = out.as_chunks_mut::<4>();
            for (pixel, out) in pixels.iter().zip(outs) {
                let sample = |i: usize| u16::from_le_bytes([pixel[2 * i], pixel[2 * i + 1]]);
                *out = self.pixel([sample(0), sample(1), sample(2)]);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn pq_matches_known_levels() {
        assert_eq!(pq_to_nits(0.0), 0.0);
        assert!(close(pq_to_nits(1.0), 10000.0, 1.0));
        // Published PQ code values for 100 and 203 nits.
        assert!(close(pq_to_nits(0.5081), 100.0, 0.5));
        assert!(close(pq_to_nits(0.5806), 203.0, 1.0));
    }

    #[test]
    fn hlg_reference_white_is_203_nits() {
        assert!(close(hlg_to_scene(0.5), 1.0 / 12.0, 1e-6));
        assert!(close(hlg_to_scene(1.0), 1.0, 1e-3));
        // BT.2408: 75% HLG grey is reference white on a 1000-nit display.
        let grey = hlg_to_scene(0.75);
        let nits = hlg_scene_to_nits([grey; 3]);
        assert!(close(nits[1], REFERENCE_WHITE_NITS, 3.0), "{nits:?}");
    }

    #[test]
    fn white_stays_white_across_primaries() {
        for c in bt2020_to_bt709([1.0; 3])
            .into_iter()
            .chain(bt709_to_bt2020([1.0; 3]))
        {
            assert!(close(c, 1.0, 1e-3));
        }
        let color = [0.8, 0.3, 0.1];
        for (a, b) in bt2020_to_bt709(bt709_to_bt2020(color))
            .into_iter()
            .zip(color)
        {
            assert!(close(a, b, 1e-4), "{a} {b}");
        }
    }

    #[test]
    fn encodes_light_back_into_signals() {
        for signal in [0.0, 0.1, 0.5081, 0.5806, 0.9, 1.0] {
            assert!(
                close(nits_to_pq(pq_to_nits(signal)), signal, 1e-4),
                "{signal}"
            );
            assert!(
                close(scene_to_hlg(hlg_to_scene(signal)), signal, 1e-4),
                "{signal}"
            );
        }
        for srgb in [0.0, 0.02, 0.5, 1.0] {
            assert!(close(srgb_encode(srgb_decode(srgb)), srgb, 1e-5), "{srgb}");
        }
        let scene = [0.3, 0.2, 0.1];
        let back = hlg_nits_to_scene(hlg_scene_to_nits(scene));
        for (a, b) in back.into_iter().zip(scene) {
            assert!(close(a, b, 1e-5), "{a} {b}");
        }
        // SDR white lands on HDR reference white.
        let white = sdr_to_hdr([1.0; 3], Transfer::Pq);
        assert!(white.iter().all(|&c| close(c, 0.5806, 1e-3)), "{white:?}");
        let white = sdr_to_hdr([1.0; 3], Transfer::Hlg);
        assert!(white.iter().all(|&c| close(c, 0.75, 5e-3)), "{white:?}");
        assert_eq!(sdr_to_hdr([0.0; 3], Transfer::Hlg), [0.0; 3]);
    }

    #[test]
    fn tone_curve_is_smooth_and_bounded() {
        for headroom in [1.0, 2.5, 16.0] {
            let knee = KNEE * headroom;
            assert_eq!(tone_curve(knee * 0.5, headroom), knee * 0.5);
            assert!(close(tone_curve(knee + 1e-4, headroom), knee + 1e-4, 1e-5));
            let mut last = 0.0;
            for i in 1..2000 {
                let y = tone_curve(i as f32 * 0.05, headroom);
                // Far above the knee it rounds to the headroom itself.
                assert!(y >= last && y <= headroom, "{headroom}: {y} after {last}");
                last = y;
            }
        }
    }

    #[test]
    fn tone_map_keeps_hue() {
        let [r, g, b] = tone_map([2.0, 1.0, 0.5], 1.0);
        assert!(r < 1.0);
        assert!(close(g / r, 0.5, 1e-6) && close(b / r, 0.25, 1e-6));
    }

    #[test]
    fn maps_hdr_greys_to_sdr() {
        let code = |signal: f32| (signal * 65535.0).round() as u16;
        for transfer in [Transfer::Pq, Transfer::Hlg] {
            let mapper = ToneMapper::new(transfer);
            assert_eq!(mapper.pixel([0; 3]), [0, 0, 0, 255]);
            // Reference white comes out just under SDR white...
            let white = match transfer {
                Transfer::Pq => 0.5806,
                Transfer::Hlg => 0.75,
            };
            let [b, g, r, _] = mapper.pixel([code(white); 3]);
            assert!(
                r == g && g == b && (235..=250).contains(&r),
                "{transfer:?}: {r}"
            );
            // ...and the brightest highlights just under the top, not clipped
            // far below it.
            let [b, ..] = mapper.pixel([u16::MAX; 3]);
            assert!((250..=255).contains(&b), "{transfer:?}: {b}");
        }
    }

    #[test]
    fn applies_gamma_after_tone_mapping() {
        let mut mapper = ToneMapper::new(Transfer::Pq);
        let code = |signal: f32| (signal * 65535.0).round() as u16;
        // A grey a stop under reference white.
        let grey = [code(0.5); 3];
        let plain = mapper.pixel(grey)[1];
        mapper.set_gamma(1.5);
        assert!(mapper.pixel(grey)[1] > plain + 20);
        assert_eq!(mapper.pixel([0; 3]), [0, 0, 0, 255]);
        mapper.set_gamma(1.0);
        assert_eq!(mapper.pixel(grey)[1], plain);
    }

    /// Signals and the sRGB bytes they must come out as, worked out apart
    /// from this code from the standards: ST 2084 for PQ, BT.2100 for HLG
    /// with its OOTF on a 1000-nit display, BT.2087's matrix from BT.2020 to
    /// BT.709, sRGB's encoding, and Plays' own tone curve above 0.75 of SDR
    /// white. A byte either way is rounding.
    #[test]
    fn lands_known_signals_on_known_srgb() {
        let cases: &[(Transfer, [f32; 3], [u8; 3], &str)] = &[
            (Transfer::Pq, [0.25; 3], [44; 3], "5 nits"),
            (Transfer::Pq, [0.4; 3], [111; 3], "32 nits"),
            (Transfer::Pq, [0.5081; 3], [186; 3], "100 nits"),
            (
                Transfer::Pq,
                [0.5806; 3],
                [244; 3],
                "reference white, eased",
            ),
            (Transfer::Pq, [0.65; 3], [255; 3], "390 nits, white"),
            (Transfer::Hlg, [0.25; 3], [61; 3], "10 nits"),
            (Transfer::Hlg, [0.5; 3], [137; 3], "51 nits"),
            (Transfer::Hlg, [0.75; 3], [244; 3], "reference white, eased"),
            (Transfer::Hlg, [0.9; 3], [255; 3], "522 nits, white"),
            (
                Transfer::Pq,
                [0.46207, 0.27135, 0.17665],
                [186, 0, 0],
                "BT.709 red at 100 nits",
            ),
            (
                Transfer::Hlg,
                [0.29336, 0.4902, 0.15165],
                [0, 136, 0],
                "BT.709 green at 50 nits",
            ),
        ];
        let code = |signal: f32| (signal * 65535.0).round() as u16;
        for &(transfer, signal, expected, what) in cases {
            let [b, g, r, a] = ToneMapper::new(transfer).pixel(signal.map(code));
            let near = [r, g, b]
                .iter()
                .zip(expected)
                .all(|(&got, want)| got.abs_diff(want) <= 1);
            assert!(
                near && a == 255,
                "{transfer:?} {what}: {:?}, not {expected:?}",
                [r, g, b]
            );
        }
    }

    /// Gamma bends the sRGB signal after tone mapping: worked out as above.
    #[test]
    fn known_signals_with_gamma() {
        let code = |signal: f32| (signal * 65535.0).round() as u16;
        let cases = [
            (Transfer::Pq, 0.4, 1.5, 147),
            (Transfer::Pq, 0.4, 0.8, 90),
            (Transfer::Pq, 0.5806, 1.5, 248),
            (Transfer::Hlg, 0.5, 1.5, 168),
            (Transfer::Hlg, 0.5, 0.8, 117),
        ];
        for (transfer, signal, gamma, expected) in cases {
            let mut mapper = ToneMapper::new(transfer);
            mapper.set_gamma(gamma);
            let [_, g, ..] = mapper.pixel([code(signal); 3]);
            assert!(
                g.abs_diff(expected) <= 1,
                "{transfer:?} {signal} at gamma {gamma}: {g}, not {expected}"
            );
        }
    }

    #[test]
    fn reads_pq_between_table_entries() {
        let mapper = ToneMapper::new(Transfer::Pq);
        for signal in [0.0, 0.1234, 0.5806, 0.9, 1.0] {
            let exact = pq_to_nits(signal) / REFERENCE_WHITE_NITS;
            let read = mapper.pq_light(signal);
            assert!(
                close(read, exact, exact * 1e-3 + 1e-7),
                "{signal}: {read} {exact}"
            );
        }
    }

    #[test]
    fn runs_over_rows_with_padding() {
        let mapper = ToneMapper::new(Transfer::Pq);
        let (width, height, stride) = (3, 200, 3 * 6 + 10);
        let mut data = vec![0u8; stride * height];
        // The last pixel of the last row is PQ reference white.
        let white = ((0.5806f32 * 65535.0).round() as u16).to_le_bytes();
        let last = (height - 1) * stride + (width - 1) * 6;
        for i in 0..3 {
            data[last + 2 * i..][..2].copy_from_slice(&white);
        }
        let out = mapper.run(&data, stride, width, height);
        assert_eq!(out.len(), width * height * 4);
        assert_eq!(&out[..4], &[0, 0, 0, 255]);
        assert!(out[out.len() - 4] > 230);
    }
}
