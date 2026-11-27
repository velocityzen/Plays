//! How HDR codes light, both ways: PQ's and HLG's curves, BT.2020's
//! primaries and BT.709's, and sRGB's encoding.

use crate::{
    BT2020_LUMA, HLG_A, HLG_B, HLG_C, HLG_GAMMA, HLG_PEAK_NITS, PQ_C1, PQ_C2, PQ_C3, PQ_M1, PQ_M2,
    PQ_PEAK_NITS, REFERENCE_WHITE_NITS, Transfer,
};

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

/// PQ's EOTF: a signal in 0..=1 to nits.
pub fn pq_to_nits(signal: f32) -> f32 {
    let p = signal.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    PQ_PEAK_NITS * ((p - PQ_C1).max(0.0) / (PQ_C2 - PQ_C3 * p)).powf(1.0 / PQ_M1)
}

/// PQ's inverse EOTF: nits to a signal in 0..=1.
pub fn nits_to_pq(nits: f32) -> f32 {
    let y = (nits / PQ_PEAK_NITS).clamp(0.0, 1.0).powf(PQ_M1);
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
    BT2020_LUMA[0] * rgb[0] + BT2020_LUMA[1] * rgb[1] + BT2020_LUMA[2] * rgb[2]
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

#[cfg(test)]
#[path = "tests/light.rs"]
mod tests;
