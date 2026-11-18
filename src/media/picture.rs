//! Decoded pictures in the forms gpui can draw, and the gamma every picture
//! is shown with.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

use gpui::{ImageId, RenderImage};

#[cfg(target_os = "macos")]
pub use super::videotoolbox::Surface;
#[cfg(not(target_os = "macos"))]
pub use super::yuv::YuvPicture;

/// A decoded picture, ready to draw.
#[derive(Clone)]
pub enum Picture {
    /// BGRA pixels in memory; gpui uploads them to its texture atlas when
    /// drawn, and they must be dropped from it once off screen.
    Image(Arc<RenderImage>),
    /// A hardware-decoded frame that stays in GPU memory and is drawn as is.
    #[cfg(target_os = "macos")]
    Surface(Surface),
    /// A frame's Y'CbCr planes, which gpui uploads and converts on the GPU
    /// as it draws them, with the gamma then.
    #[cfg(not(target_os = "macos"))]
    Yuv(YuvPicture),
}

/// Identifies a picture, to tell whether it is the one that was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureId {
    Image(ImageId),
    #[cfg(target_os = "macos")]
    Surface(u64),
    #[cfg(not(target_os = "macos"))]
    Yuv(u64),
}

impl Picture {
    /// Its width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Picture::Image(image) => {
                let size = image.size(0);
                (size.width.0.max(0) as u32, size.height.0.max(0) as u32)
            }
            #[cfg(target_os = "macos")]
            Picture::Surface(surface) => surface.size(),
            #[cfg(not(target_os = "macos"))]
            Picture::Yuv(yuv) => yuv.size(),
        }
    }

    /// Dolby Vision on a surface, which only gpui's own shader shows right.
    pub fn is_dolby_vision(&self) -> bool {
        match self {
            #[cfg(target_os = "macos")]
            Picture::Surface(surface) => surface.is_dolby_vision(),
            _ => false,
        }
    }

    pub fn id(&self) -> PictureId {
        match self {
            Picture::Image(image) => PictureId::Image(image.id),
            #[cfg(target_os = "macos")]
            Picture::Surface(surface) => PictureId::Surface(surface.id()),
            #[cfg(not(target_os = "macos"))]
            Picture::Yuv(yuv) => PictureId::Yuv(yuv.frame.id()),
        }
    }
}

/// Gamma for SDR and HDR video, the same for every player: `[sdr, hdr]`,
/// as f32 bits. 1 leaves pictures as they are.
static GAMMA: [AtomicU32; 2] = [
    AtomicU32::new(1f32.to_bits()),
    AtomicU32::new(1f32.to_bits()),
];

/// Set the gamma of SDR and of HDR video (Dolby Vision included). Decoders
/// use it for every picture they convert from then on; surfaces and planes
/// carry it to the GPU when drawn.
pub fn set_gamma(sdr: f32, hdr: f32) {
    GAMMA[0].store(sdr.to_bits(), Ordering::Relaxed);
    GAMMA[1].store(hdr.to_bits(), Ordering::Relaxed);
}

/// The gamma for HDR video, or for SDR video.
pub fn gamma(hdr: bool) -> f32 {
    f32::from_bits(GAMMA[usize::from(hdr)].load(Ordering::Relaxed))
}

/// Gamma bends an encoded signal between black and SDR white, which stay
/// put: `signal^(1/gamma)`. Above 1 lifts shadows and midtones, below 1
/// sinks them. Signals above white (HDR highlights) bend the same way.
pub fn apply_gamma(signal: f32, gamma: f32) -> f32 {
    signal.signum() * signal.abs().powf(1.0 / gamma)
}

/// Gamma for 8-bit samples, as a lookup table.
pub fn gamma_table(gamma: f32) -> [u8; 256] {
    std::array::from_fn(|i| (apply_gamma(i as f32 / 255.0, gamma) * 255.0).round() as u8)
}

/// Make a tightly packed `width` x `height` BGRA picture, with `row` filling
/// in each row, spread over the CPU's cores.
pub fn bgra_rows(width: usize, height: usize, row: impl Fn(usize, &mut [u8]) + Sync) -> Vec<u8> {
    rows(width * 4, height, row)
}

/// Make `height` rows of `row_len` items each, with `row` filling in each
/// row, spread over the CPU's cores.
pub fn rows<T: Clone + Default + Send>(
    row_len: usize,
    height: usize,
    row: impl Fn(usize, &mut [T]) + Sync,
) -> Vec<T> {
    let mut out = vec![T::default(); row_len * height];
    if out.is_empty() {
        return out;
    }
    let threads = thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
        .min(height.div_ceil(64))
        .max(1);
    let rows_per_thread = height.div_ceil(threads);
    let row = &row;
    thread::scope(|scope| {
        for (chunk, out) in out.chunks_mut(rows_per_thread * row_len).enumerate() {
            let first_row = chunk * rows_per_thread;
            scope.spawn(move || {
                for (y, out) in out.chunks_exact_mut(row_len).enumerate() {
                    row(first_row + y, out);
                }
            });
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_keeps_black_and_white() {
        for gamma in [0.5, 1.0, 1.4, 2.0] {
            let table = gamma_table(gamma);
            assert_eq!((table[0], table[255]), (0, 255));
        }
        assert_eq!(gamma_table(1.0), std::array::from_fn(|i| i as u8));
        // Above 1 lifts the middle, below 1 sinks it.
        assert!(gamma_table(1.5)[128] > 150);
        assert!(gamma_table(0.67)[128] < 100);
        // HDR highlights bend the other way round, toward white.
        assert!(apply_gamma(2.0, 1.5) < 2.0 && apply_gamma(2.0, 0.67) > 2.0);
        assert_eq!(apply_gamma(-0.25, 2.0), -0.5);
    }
}
