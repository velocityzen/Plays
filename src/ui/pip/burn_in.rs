//! Subtitles burned into the frames Picture in Picture shows. Its window
//! shows the video layer's own pixels and nothing drawn over them, so the
//! subtitles showing go into the frames. They are drawn once into an
//! `Overlay`, in the frames' signal and premultiplied, then laid over each
//! frame's samples while they show: luma by pixel, chroma by the average of
//! the pixels each sample covers.

use crate::media::hdr::{self, Transfer};
use crate::ui::overlay::Target;

/// Subtitles drawn at a frame's size, in its signal: premultiplied R'G'B'
/// and alpha, by row. Rows nothing covers have none.
pub struct Overlay {
    width: usize,
    height: usize,
    /// The frames' HDR signal, or `None` for SDR (sRGB as it is).
    transfer: Option<Transfer>,
    rows: Vec<Option<Box<[[f32; 4]]>>>,
}

impl Overlay {
    pub fn new(width: usize, height: usize, transfer: Option<Transfer>) -> Self {
        Self {
            width,
            height,
            transfer,
            rows: vec![None; height],
        }
    }

    /// Lay it over 8-bit BGRA pixels, `stride` bytes a row.
    pub fn over_bgra(&self, pixels: &mut [u8], stride: usize) {
        for (y, row) in self.covered() {
            let Some(line) = pixels.get_mut(y * stride..y * stride + self.width * 4) else {
                return;
            };
            for (pixel, &[r, g, b, a]) in line.as_chunks_mut::<4>().0.iter_mut().zip(row.iter()) {
                if a <= 0.0 {
                    continue;
                }
                for (sample, value) in pixel.iter_mut().zip([b, g, r]) {
                    let mixed = value + (1.0 - a) * f32::from(*sample) / 255.0;
                    *sample = (mixed.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
    }

    /// Lay it over biplanar 4:2:0 Y'CbCr: a luma plane, and a plane of Cb
    /// and Cr pairs at half the size each way; strides in samples.
    pub fn over_ycbcr<S: Sample>(
        &self,
        luma: &mut [S],
        luma_stride: usize,
        chroma: &mut [S],
        chroma_stride: usize,
        ycc: Ycc,
    ) {
        let (luma_range, chroma_range) = (ycc.luma_range(), ycc.chroma_range());
        for (y, row) in self.covered() {
            let Some(line) = luma.get_mut(y * luma_stride..y * luma_stride + self.width) else {
                return;
            };
            for (sample, &[r, g, b, a]) in line.iter_mut().zip(row.iter()) {
                if a > 0.0 {
                    let value = ycc.luma([r, g, b]) + (1.0 - a) * ycc.read(*sample, luma_range);
                    *sample = ycc.write(value, luma_range);
                }
            }
        }
        for chroma_y in 0..self.height.div_ceil(2) {
            let pair = [2 * chroma_y, 2 * chroma_y + 1].map(|y| self.rows.get(y));
            // Rows past the bottom edge aren't pixels; uncovered ones are.
            let tall = pair.iter().filter(|row| row.is_some()).count() as f32;
            let rows: Vec<&[[f32; 4]]> = pair
                .into_iter()
                .flatten()
                .filter_map(|row| row.as_deref())
                .collect();
            if rows.is_empty() {
                continue;
            }
            let start = chroma_y * chroma_stride;
            let Some(line) = chroma.get_mut(start..start + self.width.div_ceil(2) * 2) else {
                return;
            };
            for (chroma_x, samples) in line.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let columns = 2 * chroma_x..(2 * chroma_x + 2).min(self.width);
                let wide = columns.len() as f32;
                let mut sum = [0.0f32; 4];
                for row in &rows {
                    for pixel in &row[columns.clone()] {
                        for (total, value) in sum.iter_mut().zip(pixel) {
                            *total += value;
                        }
                    }
                }
                let [r, g, b, a] = sum.map(|total| total / (wide * tall));
                if a <= 0.0 {
                    continue;
                }
                let (cb, cr) = ycc.chroma([r, g, b]);
                for (sample, value) in samples.iter_mut().zip([cb, cr]) {
                    let mixed = value + (1.0 - a) * ycc.read(*sample, chroma_range);
                    *sample = ycc.write(mixed, chroma_range);
                }
            }
        }
    }

    /// The rows something covers, by number.
    fn covered(&self) -> impl Iterator<Item = (usize, &[[f32; 4]])> {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(y, row)| row.as_deref().map(|row| (y, row)))
    }
}

impl Target for Overlay {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn signal(&self, srgb: [f32; 3]) -> [f32; 3] {
        match self.transfer {
            None => srgb,
            Some(transfer) => hdr::sdr_to_hdr(srgb, transfer),
        }
    }

    fn blend(&mut self, x: usize, y: usize, signal: [f32; 3], alpha: f32) {
        if alpha <= 0.0 || x >= self.width {
            return;
        }
        let width = self.width;
        let Some(row) = self.rows.get_mut(y) else {
            return;
        };
        let pixel = &mut row.get_or_insert_with(|| vec![[0.0; 4]; width].into_boxed_slice())[x];
        let alpha = alpha.min(1.0);
        for (value, signal) in pixel.iter_mut().zip(signal) {
            *value = signal * alpha + *value * (1.0 - alpha);
        }
        pixel[3] = alpha + pixel[3] * (1.0 - alpha);
    }
}

/// A sample of a Y'CbCr plane, as its container holds it.
pub trait Sample: Copy {
    fn code(self) -> u32;
    fn from_code(code: u32) -> Self;
}

impl Sample for u8 {
    fn code(self) -> u32 {
        self.into()
    }

    fn from_code(code: u32) -> Self {
        code.min(255) as u8
    }
}

impl Sample for u16 {
    fn code(self) -> u32 {
        self.into()
    }

    fn from_code(code: u32) -> Self {
        code.min(65535) as u16
    }
}

/// How R'G'B' goes to Y'CbCr samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ycc {
    /// The matrix's red and blue weights.
    pub kr: f32,
    pub kb: f32,
    pub bits: u32,
    pub full_range: bool,
    /// How far up their containers the samples sit: 6 for 10 bits in 16.
    pub shift: u32,
}

impl Ycc {
    pub const BT601: (f32, f32) = (0.299, 0.114);
    pub const BT709: (f32, f32) = (0.2126, 0.0722);
    pub const BT2020: (f32, f32) = (0.2627, 0.0593);

    fn max(&self) -> f32 {
        ((1u32 << self.bits) - 1) as f32
    }

    /// Black's code and how far white is from it.
    fn luma_range(&self) -> (f32, f32) {
        let unit = (1u32 << (self.bits - 8)) as f32;
        if self.full_range {
            (0.0, self.max())
        } else {
            (16.0 * unit, 219.0 * unit)
        }
    }

    /// No color's code and how far the extremes are apart.
    fn chroma_range(&self) -> (f32, f32) {
        let unit = (1u32 << (self.bits - 8)) as f32;
        let scale = if self.full_range {
            self.max()
        } else {
            224.0 * unit
        };
        (128.0 * unit, scale)
    }

    fn read(&self, sample: impl Sample, (offset, scale): (f32, f32)) -> f32 {
        ((sample.code() >> self.shift) as f32 - offset) / scale
    }

    fn write<S: Sample>(&self, value: f32, (offset, scale): (f32, f32)) -> S {
        let code = (offset + value * scale).round().clamp(0.0, self.max());
        S::from_code((code as u32) << self.shift)
    }

    fn luma(&self, [r, g, b]: [f32; 3]) -> f32 {
        self.kr * r + (1.0 - self.kr - self.kb) * g + self.kb * b
    }

    fn chroma(&self, rgb: [f32; 3]) -> (f32, f32) {
        let y = self.luma(rgb);
        let [r, _, b] = rgb;
        (
            (b - y) / (2.0 * (1.0 - self.kb)),
            (r - y) / (2.0 * (1.0 - self.kr)),
        )
    }
}

#[cfg(test)]
#[path = "tests/burn_in.rs"]
mod tests;
