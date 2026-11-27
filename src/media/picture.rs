//! Decoded pictures in the forms gpui can draw, and the gamma every picture
//! is shown with.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;

use gpui::{ImageId, RenderImage};
use rayon::prelude::*;

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
    // Zeros are asked of the system (calloc), whose fresh pages are.
    let mut out = vec![T::default(); row_len * height];
    in_rows(&mut out, row_len, row);
    out
}

/// Threads at most that share out a picture's rows.
const MOST_BANDS: usize = 8;
/// Rows at least that are worth a thread.
const BAND_ROWS: usize = 64;

/// The threads that convert pictures, kept from one frame to the next;
/// none, should the system refuse them.
fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = thread::available_parallelism().map_or(1, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads.min(MOST_BANDS))
            .thread_name(|n| format!("pictures {n}"))
            .build()
            .inspect_err(|err| log::warn!("converting pictures on one thread: {err}"))
            .ok()
    })
    .as_ref()
}

/// Run `row` over each of the rows of `row_len` items `pixels` holds, in
/// bands of them spread over the cores. The last may be shorter, as a
/// frame's plane's is, to the end of its picture.
pub fn in_rows<T: Send>(pixels: &mut [T], row_len: usize, row: impl Fn(usize, &mut [T]) + Sync) {
    if row_len == 0 || pixels.is_empty() {
        return;
    }
    let height = pixels.len().div_ceil(row_len);
    let band = |(band, out): (usize, &mut [T]), band_rows: usize| {
        for (y, out) in out.chunks_mut(row_len).enumerate() {
            row(band * band_rows + y, out);
        }
    };
    let Some(pool) = pool() else {
        band((0, pixels), height);
        return;
    };
    let bands = pool
        .current_num_threads()
        .min(height.div_ceil(BAND_ROWS))
        .max(1);
    let band_rows = height.div_ceil(bands);
    pool.install(|| {
        pixels
            .par_chunks_mut(band_rows * row_len)
            .enumerate()
            .for_each(|out| band(out, band_rows));
    });
}

#[cfg(test)]
#[path = "tests/picture.rs"]
mod tests;
