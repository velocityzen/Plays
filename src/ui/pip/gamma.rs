//! Gamma bent into the frames Picture in Picture shows. The block's shader
//! bends a VideoToolbox surface's signal as it draws it; the window shows
//! the frames' own samples, so they are bent here once they are scaled to
//! it: R'G'B' channel by channel, through a table, then back to Y'CbCr,
//! luma by pixel and chroma by the average of the pixels each sample
//! covers.

use rayon::prelude::*;

use super::burn_in::{Sample, Ycc};
use crate::media::hdr::{self, REFERENCE_WHITE_NITS, Transfer};
use crate::media::picture::apply_gamma;

/// Steps in a curve's table, over signals from 0 to 1.
const STEPS: usize = 1024;

/// What gamma makes of each signal, for frames of one kind.
pub struct Curve {
    /// What the signals `0, 1 / STEPS, …, 1` become.
    table: Box<[f32]>,
}

impl Curve {
    /// `gamma` for frames in `transfer`'s signal, or in SDR's.
    pub fn new(gamma: f32, transfer: Option<Transfer>) -> Self {
        let table = (0..=STEPS)
            .map(|step| bend(step as f32 / STEPS as f32, gamma, transfer))
            .collect();
        Self { table }
    }

    /// What `signal` becomes, between the table's steps; past 0 and 1 it
    /// is as they are.
    pub fn at(&self, signal: f32) -> f32 {
        let place = signal.clamp(0.0, 1.0) * STEPS as f32;
        let step = (place as usize).min(STEPS - 1);
        let (low, high) = (self.table[step], self.table[step + 1]);
        low + (high - low) * (place - step as f32)
    }

    /// Bend 8-bit BGRA pixels, `width` a row and `stride` bytes apart.
    pub fn bend_bgra(&self, pixels: &mut [u8], stride: usize, width: usize) {
        let table: [u8; 256] =
            std::array::from_fn(|code| (self.at(code as f32 / 255.0) * 255.0).round() as u8);
        pixels.par_chunks_mut(stride.max(1)).for_each(|row| {
            let Some(row) = row.get_mut(..width * 4) else {
                return;
            };
            for pixel in row.as_chunks_mut::<4>().0 {
                for sample in &mut pixel[..3] {
                    *sample = table[usize::from(*sample)];
                }
            }
        });
    }

    /// Bend biplanar 4:2:0 Y'CbCr `width` pixels wide: a luma plane, and a
    /// plane of Cb and Cr pairs at half the size each way; strides in
    /// samples.
    pub fn bend_ycbcr<S: Sample + Send>(
        &self,
        (luma, luma_stride): (&mut [S], usize),
        (chroma, chroma_stride): (&mut [S], usize),
        width: usize,
        ycc: Ycc,
    ) {
        let (luma_range, chroma_range) = (ycc.luma_range(), ycc.chroma_range());
        let rows = luma.par_chunks_mut(2 * luma_stride.max(1));
        let chroma_rows = chroma.par_chunks_mut(chroma_stride.max(1));
        rows.zip(chroma_rows).for_each(|(pair, line)| {
            let mut pair: Vec<&mut [S]> = pair
                .chunks_mut(luma_stride.max(1))
                .filter_map(|row| row.get_mut(..width))
                .collect();
            let Some(line) = line.get_mut(..width.div_ceil(2) * 2) else {
                return;
            };
            for (x, samples) in line.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let cb = ycc.read(samples[0], chroma_range);
                let cr = ycc.read(samples[1], chroma_range);
                let columns = 2 * x..(2 * x + 2).min(width);
                let mut sum = [0.0f32; 3];
                let mut count = 0.0;
                for row in &mut pair {
                    for sample in &mut row[columns.clone()] {
                        let rgb = ycc
                            .rgb(ycc.read(*sample, luma_range), cb, cr)
                            .map(|signal| self.at(signal));
                        *sample = ycc.write(ycc.luma(rgb), luma_range);
                        for (total, value) in sum.iter_mut().zip(rgb) {
                            *total += value;
                        }
                        count += 1.0;
                    }
                }
                if count > 0.0 {
                    let (cb, cr) = ycc.chroma(sum.map(|total| total / count));
                    samples[0] = ycc.write(cb, chroma_range);
                    samples[1] = ycc.write(cr, chroma_range);
                }
            }
        });
    }
}

/// `signal` (R', G' or B') with `gamma`, as the block's shader bends it:
/// SDR's as it is, HDR's in sRGB's encoding of its light relative to SDR
/// white, as a gray of that signal's light would be.
fn bend(signal: f32, gamma: f32, transfer: Option<Transfer>) -> f32 {
    match transfer {
        None => apply_gamma(signal, gamma),
        Some(Transfer::Pq) => {
            let light = hdr::pq_to_nits(signal) / REFERENCE_WHITE_NITS;
            hdr::nits_to_pq(bent(light, gamma) * REFERENCE_WHITE_NITS)
        }
        Some(Transfer::Hlg) => {
            let [nits, ..] = hdr::hlg_scene_to_nits([hdr::hlg_to_scene(signal); 3]);
            let light = bent(nits / REFERENCE_WHITE_NITS, gamma) * REFERENCE_WHITE_NITS;
            let [scene, ..] = hdr::hlg_nits_to_scene([light; 3]);
            hdr::scene_to_hlg(scene)
        }
    }
}

/// Light relative to SDR white, bent in sRGB's encoding of it.
fn bent(light: f32, gamma: f32) -> f32 {
    hdr::srgb_decode(apply_gamma(hdr::srgb_encode(light), gamma))
}

#[cfg(test)]
#[path = "tests/gamma.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/gamma_properties.rs"]
mod properties;
