//! Dolby Vision reshaped into plain HDR10 for Picture in Picture. A Dolby
//! Vision surface's samples are IPT, which only the block's shader reshapes
//! (`media::dovi`), and AVKit shows a frame by its tags. So once a frame is
//! scaled to the window, each pixel is reshaped as the shader reshapes it,
//! its chroma read between samples as the shader's sampler reads it, into
//! linear BT.2020 light, then encoded as PQ and written back as Y'CbCr:
//! luma by pixel, chroma by the average of the pixels each sample covers.

use rayon::prelude::*;

use super::burn_in::{Sample, Ycc};
use crate::media::dovi::Reshaping;
use crate::media::hdr::Tables;

/// Reshape biplanar 4:2:0 Dolby Vision `width` pixels wide, a luma plane
/// and a plane of chroma pairs at half the size each way (strides in
/// samples), in place into PQ Y'CbCr as `ycc` writes it.
pub fn reshape<S: Sample + Send + Sync>(
    reshaping: &Reshaping,
    (luma, luma_stride): (&mut [S], usize),
    (chroma, chroma_stride): (&mut [S], usize),
    width: usize,
    ycc: Ycc,
) {
    let (luma_stride, chroma_stride) = (luma_stride.max(1), chroma_stride.max(1));
    let height = luma.len() / luma_stride;
    let (chroma_width, chroma_height) = (width.div_ceil(2), chroma.len() / chroma_stride);
    if width == 0 || height == 0 || chroma_height == 0 || chroma_stride < chroma_width * 2 {
        return;
    }
    // Each sample is its code over its container's: the shader reads them
    // so, and the codes sit at the top.
    let unit = 1.0 / (1u64 << (8 * size_of::<S>())) as f32;
    // Read as they were, though each pair of rows writes its own.
    let pairs: Vec<[f32; 2]> = chroma
        .chunks_exact(chroma_stride)
        .flat_map(|line| line[..chroma_width * 2].as_chunks::<2>().0)
        .map(|pair| pair.map(|sample| sample.code() as f32 * unit))
        .collect();
    let chroma_at = |line: usize, x: usize| pairs[line * chroma_width + x];
    let across: Vec<_> = (0..width)
        .map(|x| sampled(x, width, chroma_width))
        .collect();
    let (luma_range, chroma_range) = (ycc.luma_range(), ycc.chroma_range());
    let tables = Tables::get();
    let rows = luma.par_chunks_mut(2 * luma_stride);
    let lines = chroma.par_chunks_mut(chroma_stride);
    rows.zip(lines).enumerate().for_each(|(at, (pair, line))| {
        let Some(line) = line.get_mut(..chroma_width * 2) else {
            return;
        };
        let mut sums = vec![([0.0f32; 3], 0.0f32); chroma_width];
        for (offset, row) in pair.chunks_mut(luma_stride).enumerate() {
            let y = 2 * at + offset;
            let Some(row) = row.get_mut(..width) else {
                continue;
            };
            let (above, below, down) = sampled(y, height, chroma_height);
            for (x, sample) in row.iter_mut().enumerate() {
                let (left, right, along) = across[x];
                let [cb, cr] = lerp(
                    lerp(chroma_at(above, left), chroma_at(above, right), along),
                    lerp(chroma_at(below, left), chroma_at(below, right), along),
                    down,
                );
                let lms = reshaping.lms_signal([sample.code() as f32 * unit, cb, cr]);
                let light = reshaping.lms_to_rgb(lms.map(|signal| tables.pq_nits(signal)));
                let rgb = light.map(|nits| tables.pq_signal(nits));
                *sample = ycc.write(ycc.luma(rgb), luma_range);
                let (sum, count) = &mut sums[x / 2];
                for (total, value) in sum.iter_mut().zip(rgb) {
                    *total += value;
                }
                *count += 1.0;
            }
        }
        for (samples, (sum, count)) in line.as_chunks_mut::<2>().0.iter_mut().zip(sums) {
            if count > 0.0 {
                let (cb, cr) = ycc.chroma(sum.map(|total| total / count));
                samples[0] = ycc.write(cb, chroma_range);
                samples[1] = ycc.write(cr, chroma_range);
            }
        }
    });
}

/// A pair of values `t` of the way from `a` to `b`.
fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Where pixel `at` of `pixels` falls among `samples` spread over the same
/// length, centers lined up and held at the edges, as a GPU's linear
/// sampler reads them: the samples either side, and how far to the second.
fn sampled(at: usize, pixels: usize, samples: usize) -> (usize, usize, f32) {
    let last = samples.saturating_sub(1);
    let place = ((at as f32 + 0.5) * samples as f32 / pixels as f32 - 0.5).clamp(0.0, last as f32);
    let low = (place as usize).min(last);
    (low, (low + 1).min(last), place - low as f32)
}

#[cfg(test)]
#[path = "tests/reshape.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/reshape_properties.rs"]
mod properties;
