//! What a player does to its picture, done to video a cast re-encodes:
//! turned and mirrored, shown at another aspect ratio, cropped to another
//! shape (centered), and its signal bent by gamma. Each frame is cropped
//! before it's scaled, turned plane by plane once it is, and its luma
//! bent through a table, on its way to the encoder.

use ffmpeg::format::Pixel;
use ffmpeg::{ffi, frame};
use ffmpeg_next as ffmpeg;
use rayon::prelude::*;

/// What is done to a picture as it's re-encoded, as a player shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjustments {
    /// Quarter turns clockwise, 0 to 3, after mirroring.
    pub turns: u8,
    /// Mirrored left to right, before it's turned.
    pub mirrored: bool,
    /// The width over height to show the decoded picture at, before it's
    /// turned, instead of its own.
    pub aspect: Option<f64>,
    /// The shape to crop the turned picture to, centered.
    pub crop: Option<f64>,
    /// Bends the encoded signal between black and white, which stay put:
    /// `signal^(1/gamma)`.
    pub gamma: f64,
}

impl Default for Adjustments {
    /// The picture as it is.
    fn default() -> Self {
        Self {
            turns: 0,
            mirrored: false,
            aspect: None,
            crop: None,
            gamma: 1.0,
        }
    }
}

impl Adjustments {
    /// On its side: width and height trade places.
    fn sideways(&self) -> bool {
        self.turns % 2 == 1
    }

    /// The size a decoded `width` × `height` picture shows at: at its
    /// aspect ratio, turned, and cropped.
    pub(crate) fn shown(&self, width: u32, height: u32) -> (f64, f64) {
        let (width, height) = self.turned(width, height);
        let (across, down) = self.kept(width, height);
        (width * across, height * down)
    }

    /// The size of a decoded `width` × `height` picture at its aspect
    /// ratio, turned.
    fn turned(&self, width: u32, height: u32) -> (f64, f64) {
        let (width, height) = (f64::from(width), f64::from(height));
        let width = self.aspect.map_or(width, |aspect| height * aspect);
        if self.sideways() {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// The part of a turned `width` × `height` picture kept, across and
    /// down.
    fn kept(&self, width: f64, height: f64) -> (f64, f64) {
        match self.crop {
            Some(crop) if width / height > crop => (height * crop / width, 1.0),
            Some(crop) => (1.0, width / crop / height),
            None => (1.0, 1.0),
        }
    }

    /// What a decoded `width` × `height` picture keeps: how many pixels
    /// are cropped from its left, top, right and bottom, in even numbers
    /// for its chroma.
    pub(crate) fn cropped(&self, width: u32, height: u32) -> [u32; 4] {
        let (turned_width, turned_height) = self.turned(width, height);
        let (across, down) = self.kept(turned_width, turned_height);
        let (across, down) = if self.sideways() {
            (down, across)
        } else {
            (across, down)
        };
        let away = |size: u32, kept: f64| {
            let kept = (f64::from(size) * kept).round() as u32;
            // Half of what goes from either side, even.
            (size.saturating_sub(kept) / 4) * 2
        };
        let (side, top) = (away(width, across), away(height, down));
        [side, top, side, top]
    }

    /// Whether frames come out as they went in.
    fn is_none(&self) -> bool {
        *self == Self::default()
    }
}

/// Crop `frame` as `adjustments` say, in place: its planes start further
/// in and it is smaller.
pub(crate) fn crop(frame: &mut frame::Video, adjustments: &Adjustments) {
    let [left, top, right, bottom] = adjustments.cropped(frame.width(), frame.height());
    if left + top + right + bottom == 0 {
        return;
    }
    // SAFETY: the frame is a decoded one of ours; cropping only moves its
    // planes' starts and shrinks its size, within what it has.
    unsafe {
        let raw = frame.as_mut_ptr();
        (*raw).crop_left = left as usize;
        (*raw).crop_top = top as usize;
        (*raw).crop_right = right as usize;
        (*raw).crop_bottom = bottom as usize;
        ffi::av_frame_apply_cropping(raw, ffi::AV_FRAME_CROP_UNALIGNED as i32);
    }
}

/// The size to scale a frame to before it's turned into `width` ×
/// `height`.
pub(crate) fn before_turning(adjustments: &Adjustments, width: u32, height: u32) -> (u32, u32) {
    if adjustments.sideways() {
        (height, width)
    } else {
        (width, height)
    }
}

/// `frame`, NV12 or P010 at its scaled size, turned and mirrored as
/// `adjustments` say and its luma bent by their gamma.
pub(crate) fn finish(frame: frame::Video, adjustments: &Adjustments) -> frame::Video {
    if adjustments.is_none() {
        return frame;
    }
    let mut frame = if adjustments.turns == 0 && !adjustments.mirrored {
        frame
    } else {
        turn(&frame, adjustments.turns, adjustments.mirrored)
    };
    if adjustments.gamma != 1.0 {
        bend(&mut frame, adjustments.gamma);
    }
    frame
}

/// `frame` turned `turns` quarter turns clockwise, after mirroring if
/// `mirrored`.
fn turn(frame: &frame::Video, turns: u8, mirrored: bool) -> frame::Video {
    let (width, height) = (frame.width(), frame.height());
    let (turned_width, turned_height) = if turns % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let mut turned = frame::Video::new(frame.format(), turned_width, turned_height);
    let sample = if frame.format() == Pixel::P010LE {
        2
    } else {
        1
    };
    // Luma a sample a pixel; chroma a pair of them for a pixel of half
    // the size each way.
    for (plane, size, scale) in [(0, sample, 1), (1, sample * 2, 2)] {
        let source = Plane {
            stride: frame.stride(plane),
            size: (
                (width as usize).div_ceil(scale),
                (height as usize).div_ceil(scale),
            ),
            pixel: size,
        };
        let stride = turned.stride(plane);
        turn_plane(
            frame.data(plane),
            &source,
            turns,
            mirrored,
            turned.data_mut(plane),
            stride,
        );
    }
    turned
}

/// How a plane's pixels lie: bytes a row, pixels across and down, and
/// bytes a pixel.
pub(crate) struct Plane {
    pub stride: usize,
    pub size: (usize, usize),
    pub pixel: usize,
}

/// The pixels of `source`, as `plane` says they lie, turned `turns`
/// quarter turns clockwise after mirroring if `mirrored`, into `target`,
/// `stride` bytes a row: rows in parallel.
pub(crate) fn turn_plane(
    source: &[u8],
    plane: &Plane,
    turns: u8,
    mirrored: bool,
    target: &mut [u8],
    stride: usize,
) {
    let (width, height) = plane.size;
    let pixel = plane.pixel;
    let turned_width = if turns % 2 == 1 { height } else { width };
    let turned_height = if turns % 2 == 1 { width } else { height };
    target
        .par_chunks_mut(stride)
        .take(turned_height)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..turned_width {
                // Where the pixel came from, mirrored, before it was turned.
                let (from_x, from_y) = match turns % 4 {
                    0 => (x, y),
                    1 => (y, height - 1 - x),
                    2 => (width - 1 - x, height - 1 - y),
                    _ => (width - 1 - y, x),
                };
                let from_x = if mirrored { width - 1 - from_x } else { from_x };
                let at = from_y * plane.stride + from_x * pixel;
                row[x * pixel..(x + 1) * pixel].copy_from_slice(&source[at..at + pixel]);
            }
        });
}

/// Bend the luma of `frame`, NV12 or P010 in video range, by `gamma`.
fn bend(frame: &mut frame::Video, gamma: f64) {
    let (width, height) = (frame.width() as usize, frame.height() as usize);
    let (stride, ten_bit) = (frame.stride(0), frame.format() == Pixel::P010LE);
    let rows = frame.data_mut(0).par_chunks_mut(stride).take(height);
    if ten_bit {
        let table = table(64, 940, 1023, gamma);
        rows.for_each(|row| {
            for sample in row[..width * 2].as_chunks_mut::<2>().0 {
                let code = usize::from(u16::from_le_bytes(*sample) >> 6);
                *sample = (table[code] << 6).to_le_bytes();
            }
        });
    } else {
        let table = table(16, 235, 255, gamma);
        rows.for_each(|row| {
            for sample in &mut row[..width] {
                *sample = table[usize::from(*sample)] as u8;
            }
        });
    }
}

/// What gamma makes of each code up to `most`, with black at `black` and
/// white at `white`, which stay put: `signal^(1/gamma)`, the signal from
/// black to white. Codes past them bend the same way.
pub(crate) fn table(black: u16, white: u16, most: u16, gamma: f64) -> Vec<u16> {
    let range = f64::from(white - black);
    (0..=most)
        .map(|code| {
            let signal = (f64::from(code) - f64::from(black)) / range;
            let bent = signal.signum() * signal.abs().powf(1.0 / gamma);
            (f64::from(black) + bent * range)
                .round()
                .clamp(0.0, f64::from(most)) as u16
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/adjust.rs"]
mod tests;
