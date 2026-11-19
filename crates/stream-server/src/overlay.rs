//! Picture subtitles (PGS, DVD and the like) drawn into video as it is
//! re-encoded. The track's packets are decoded as the media is read, and
//! each frame gets what shows at its time: the pictures, their canvas
//! stretched over the frame, in the frame's own colors, blended in.
//!
//! A display set shows until the next one, or its own end where it has
//! one; a set with no pictures clears what showed.

use std::collections::VecDeque;
use std::slice;

use ffmpeg::codec::subtitle::{Bitmap, Rect};
use ffmpeg::format::Pixel;
use ffmpeg::format::context::Input;
use ffmpeg::{Packet, codec, ffi, frame};
use ffmpeg_next as ffmpeg;

use crate::Error;
use crate::encode::Colors;
use crate::subtitles::Decoded;

/// Leeway for times read from timestamps, in seconds.
const EPSILON: f64 = 1e-3;
/// How bright subtitles' white is in PQ, as BT.2408 has graphics white.
const PQ_WHITE_NITS: f64 = 203.0;
/// The scene light giving 75% of HLG's signal, BT.2408's graphics white.
const HLG_WHITE: f64 = 0.2647;

/// How a frame codes its colors, which pictures are converted to.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Coding {
    /// Luma's weights of red and blue.
    kr: f32,
    kb: f32,
    transfer: Transfer,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Transfer {
    Sdr,
    Pq,
    Hlg,
}

impl Coding {
    /// An encoding's: 8-bit ones are BT.709, 10-bit ones the source's.
    pub fn of(colors: &Colors, ten_bit: bool) -> Self {
        use ffi::AVColorSpace::*;
        use ffi::AVColorTransferCharacteristic::*;
        let (kr, kb) = match colors.space.filter(|_| ten_bit) {
            Some(AVCOL_SPC_BT2020_NCL | AVCOL_SPC_BT2020_CL) => (0.2627, 0.0593),
            Some(AVCOL_SPC_BT470BG | AVCOL_SPC_SMPTE170M) => (0.299, 0.114),
            _ => (0.2126, 0.0722),
        };
        let transfer = match colors.transfer.filter(|_| ten_bit) {
            Some(AVCOL_TRC_SMPTE2084) => Transfer::Pq,
            Some(AVCOL_TRC_ARIB_STD_B67) => Transfer::Hlg,
            _ => Transfer::Sdr,
        };
        Self { kr, kb, transfer }
    }

    /// A palette entry, `0xAARRGGBB`, as luma (0 to 1), blue and red
    /// differences (-0.5 to 0.5) and opacity, premultiplied.
    fn color(&self, argb: u32) -> [f32; 4] {
        let alpha = (argb >> 24) as f32 / 255.0;
        let channel = |shift: u32| {
            self.transfer
                .encode(f64::from((argb >> shift) & 0xFF) / 255.0)
        };
        let (r, g, b) = (channel(16), channel(8), channel(0));
        let y = self.kr * r + (1.0 - self.kr - self.kb) * g + self.kb * b;
        let cb = (b - y) / (2.0 * (1.0 - self.kb));
        let cr = (r - y) / (2.0 * (1.0 - self.kr));
        [y * alpha, cb * alpha, cr * alpha, alpha]
    }
}

impl Transfer {
    /// A picture's channel, made for SDR, in this transfer: its white at
    /// graphics white, not the brightest there is.
    fn encode(self, value: f64) -> f32 {
        let encoded = match self {
            Self::Sdr => value,
            Self::Pq => {
                // SMPTE ST 2084's inverse EOTF.
                let (m1, m2) = (2610.0 / 16384.0, 2523.0 / 4096.0 * 128.0);
                let (c1, c2, c3) = (3424.0 / 4096.0, 2413.0 / 128.0, 2392.0 / 128.0);
                let light = (value.powf(2.4) * PQ_WHITE_NITS / 10_000.0).powf(m1);
                ((c1 + c2 * light) / (1.0 + c3 * light)).powf(m2)
            }
            Self::Hlg => {
                // BT.2100's HLG OETF.
                let (a, b, c) = (0.178_832_77, 0.284_668_92, 0.559_910_73);
                let light = value.powf(2.4) * HLG_WHITE;
                if light <= 1.0 / 12.0 {
                    (3.0 * light).sqrt()
                } else {
                    a * (12.0 * light - b).ln() + c
                }
            }
        };
        encoded as f32
    }
}

/// What shows from `start`: a display set's pictures, until the next set
/// or `end`.
struct Set {
    id: u64,
    start: f64,
    end: Option<f64>,
    pictures: Vec<Picture>,
    /// The canvas they are placed on, which the frame stretches over.
    canvas: (u32, u32),
}

/// A picture of palette indices, placed on the canvas.
#[derive(Clone)]
struct Picture {
    x: i32,
    y: i32,
    width: usize,
    height: usize,
    indices: Vec<u8>,
    /// `0xAARRGGBB`.
    palette: Vec<u32>,
}

/// A picture as drawn into a frame: each pixel of a box of the frame,
/// premultiplied, as `Coding::color` gives them.
#[derive(Debug)]
struct Patch {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    pixels: Vec<[f32; 4]>,
}

/// Display sets in the order they show, from the one showing.
#[derive(Default)]
struct Sets {
    sets: VecDeque<Set>,
    made: u64,
}

impl Sets {
    fn add(&mut self, start: f64, end: Option<f64>, pictures: Vec<Picture>, canvas: (u32, u32)) {
        self.made += 1;
        let set = Set {
            id: self.made,
            start,
            end,
            pictures,
            canvas,
        };
        // In order, should a muxer have slipped one in late.
        let at = self
            .sets
            .iter()
            .rposition(|other| other.start <= start)
            .map_or(0, |index| index + 1);
        self.sets.insert(at, set);
    }

    /// The set showing at `time`, if it shows anything; those it has
    /// replaced go.
    fn at(&mut self, time: f64) -> Option<&Set> {
        while self.sets.len() > 1 && self.sets[1].start <= time + EPSILON {
            self.sets.pop_front();
        }
        self.sets.front().filter(|set| {
            set.start <= time + EPSILON
                && set.end.is_none_or(|end| time < end - EPSILON)
                && !set.pictures.is_empty()
        })
    }
}

/// A picture subtitle track, decoded as its packets are read, drawn into
/// frames.
pub(crate) struct Overlay {
    decoder: ffmpeg::decoder::Subtitle,
    stream: usize,
    time_base: f64,
    /// The canvas where the decoder doesn't say: the video's size.
    canvas: (u32, u32),
    coding: Coding,
    sets: Sets,
    /// The showing set's pictures as drawn, by its id.
    patches: Option<(u64, Vec<Patch>)>,
}

// SAFETY: the decoder is used by one thread at a time, the job's.
unsafe impl Send for Overlay {}

impl Overlay {
    /// For the subtitles in `stream` of `input`, over video of `size`,
    /// drawn into frames coded as `coding` says.
    pub fn open(
        input: &Input,
        stream: usize,
        size: (u32, u32),
        coding: Coding,
    ) -> Result<Self, Error> {
        let fail = |err: ffmpeg::Error| Error::Source(format!("can't decode the subtitles: {err}"));
        let (parameters, time_base) = {
            let stream = input
                .stream(stream)
                .ok_or_else(|| Error::Source(format!("there is no stream {stream}")))?;
            (stream.parameters(), stream.time_base())
        };
        let mut decoder = codec::context::Context::from_parameters(parameters.clone())
            .map_err(fail)?
            .decoder();
        decoder.set_packet_time_base(time_base);
        let codec = ffmpeg::decoder::find(parameters.id())
            .ok_or_else(|| Error::Source("there is no decoder for the subtitles".into()))?;
        let decoder = decoder
            .open_as(codec)
            .and_then(|opened| opened.subtitle())
            .map_err(fail)?;
        Ok(Self {
            decoder,
            stream,
            time_base: f64::from(time_base),
            canvas: size,
            coding,
            sets: Sets::default(),
            patches: None,
        })
    }

    pub fn stream(&self) -> usize {
        self.stream
    }

    /// Decode a packet of the subtitles.
    pub fn take(&mut self, packet: &Packet) {
        let mut decoded = Decoded(ffmpeg::Subtitle::new());
        match self.decoder.decode(packet, &mut decoded.0) {
            Ok(true) => {}
            Ok(false) => return,
            Err(err) => {
                log::debug!("a picture subtitle didn't decode: {err}");
                return;
            }
        }
        let subtitle = &decoded.0;
        let Some(base) = subtitle
            .pts()
            .map(|pts| pts as f64 / f64::from(ffi::AV_TIME_BASE))
            .or_else(|| packet.pts().map(|pts| pts as f64 * self.time_base))
        else {
            return;
        };
        let start = base + f64::from(subtitle.start()) / 1000.0;
        let end = match subtitle.end() {
            0 | u32::MAX if packet.duration() > 0 => {
                Some(base + packet.duration() as f64 * self.time_base)
            }
            0 | u32::MAX => None,
            end => Some(base + f64::from(end) / 1000.0),
        };
        // SAFETY: the decoder is open. PGS's sets its size from the
        // canvas each display set gives.
        let canvas = unsafe {
            let raw = &*self.decoder.as_ptr();
            (raw.width, raw.height)
        };
        let canvas = match canvas {
            (width, height) if width > 0 && height > 0 => (width as u32, height as u32),
            _ => self.canvas,
        };
        let pictures = subtitle
            .rects()
            .filter_map(|rect| match rect {
                Rect::Bitmap(bitmap) => picture(&bitmap),
                _ => None,
            })
            .collect();
        self.sets.add(start, end, pictures, canvas);
    }

    /// Blend into `frame`, NV12 or P010, what shows at `time`.
    pub fn draw(&mut self, frame: &mut frame::Video, time: f64) {
        let ten_bit = match frame.format() {
            Pixel::NV12 => false,
            Pixel::P010LE => true,
            _ => return,
        };
        let Some(set) = self.sets.at(time) else {
            return;
        };
        let size = (frame.width() as usize, frame.height() as usize);
        if self.patches.as_ref().is_none_or(|(id, _)| *id != set.id) {
            let patches = set
                .pictures
                .iter()
                .filter_map(|picture| patch(picture, set.canvas, size, &self.coding))
                .collect();
            self.patches = Some((set.id, patches));
        }
        let Some((_, patches)) = &self.patches else {
            return;
        };
        let stride = frame.stride(0);
        for patch in patches {
            blend_luma(frame.data_mut(0), stride, patch, ten_bit);
        }
        let stride = frame.stride(1);
        for patch in patches {
            blend_chroma(frame.data_mut(1), stride, patch, ten_bit);
        }
    }
}

/// A decoded rect's picture, copied out of it.
fn picture(bitmap: &Bitmap) -> Option<Picture> {
    // SAFETY: the rect belongs to a subtitle being read. Its `data[0]`
    // holds `h` rows of `w` palette indices, `linesize[0]` bytes apart,
    // and `data[1]` its `nb_colors` palette entries.
    unsafe {
        let raw = &*bitmap.as_ptr();
        let (width, height) = (raw.w.max(0) as usize, raw.h.max(0) as usize);
        let stride = raw.linesize[0].max(0) as usize;
        if width == 0 || height == 0 || stride < width {
            return None;
        }
        if raw.data[0].is_null() || raw.data[1].is_null() {
            return None;
        }
        let rows = slice::from_raw_parts(raw.data[0], stride * (height - 1) + width);
        let palette = slice::from_raw_parts(
            raw.data[1].cast::<u32>(),
            raw.nb_colors.clamp(0, 256) as usize,
        );
        let mut indices = Vec::with_capacity(width * height);
        for row in rows.chunks(stride).take(height) {
            indices.extend_from_slice(&row[..width]);
        }
        Some(Picture {
            x: raw.x,
            y: raw.y,
            width,
            height,
            indices,
            palette: palette.to_vec(),
        })
    }
}

/// `picture` as drawn into a frame of `size`, its `canvas` stretched over
/// the frame.
fn patch(
    picture: &Picture,
    canvas: (u32, u32),
    size: (usize, usize),
    coding: &Coding,
) -> Option<Patch> {
    if picture.indices.len() < picture.width * picture.height {
        return None;
    }
    let colors: Vec<[f32; 4]> = picture
        .palette
        .iter()
        .map(|&argb| coding.color(argb))
        .collect();
    let pixels: Vec<[f32; 4]> = picture
        .indices
        .iter()
        .map(|&index| colors.get(usize::from(index)).copied().unwrap_or_default())
        .collect();
    let scale = |frame: usize, canvas: u32| frame as f64 / f64::from(canvas.max(1));
    let (left, columns) = taps(
        f64::from(picture.x),
        picture.width,
        scale(size.0, canvas.0),
        size.0,
    )?;
    let (top, rows) = taps(
        f64::from(picture.y),
        picture.height,
        scale(size.1, canvas.1),
        size.1,
    )?;
    // Across, then down.
    let width = columns.len();
    let mut across = vec![[0.0f32; 4]; picture.height * width];
    for (line, out) in pixels.chunks(picture.width).zip(across.chunks_mut(width)) {
        for (weights, out) in columns.iter().zip(out) {
            *out = mix(weights
                .iter()
                .map(|&(source, weight)| (line[source], weight)));
        }
    }
    let mut pixels = Vec::with_capacity(rows.len() * width);
    for weights in &rows {
        for column in 0..width {
            pixels.push(mix(weights
                .iter()
                .map(|&(source, weight)| (across[source * width + column], weight))));
        }
    }
    Some(Patch {
        x: left,
        y: top,
        width,
        height: rows.len(),
        pixels,
    })
}

fn mix(samples: impl Iterator<Item = ([f32; 4], f32)>) -> [f32; 4] {
    let mut sum = [0.0; 4];
    for (sample, weight) in samples {
        for (sum, value) in sum.iter_mut().zip(sample) {
            *sum += value * weight;
        }
    }
    sum
}

/// For each frame pixel along an axis, the picture's pixels it takes and
/// their weights.
type Taps = Vec<Vec<(usize, f32)>>;

/// Along one axis, where a picture `length` pixels long from `start` on
/// the canvas lands in a frame `size` pixels long, at `scale` frame pixels
/// a canvas pixel: the first frame pixel it may touch, and for each from
/// there the picture's pixels it takes and their weights. Enlarged, the
/// picture is interpolated; shrunk, averaged.
fn taps(start: f64, length: usize, scale: f64, size: usize) -> Option<(usize, Taps)> {
    if length == 0 || size == 0 || scale.is_nan() || scale <= 0.0 {
        return None;
    }
    let end = start + length as f64;
    let first = ((start - 1.0) * scale).floor().max(0.0) as usize;
    let last = (((end + 1.0) * scale).ceil().max(0.0) as usize).min(size);
    if first >= last {
        return None;
    }
    let taps = (first..last)
        .map(|out| {
            if scale >= 1.0 {
                // Its center, among the picture's pixels' centers.
                let at = (out as f64 + 0.5) / scale - start - 0.5;
                let below = at.floor();
                let fraction = (at - below) as f32;
                [(below, 1.0 - fraction), (below + 1.0, fraction)]
                    .into_iter()
                    .filter(|&(source, weight)| {
                        weight > 0.0 && source >= 0.0 && source < length as f64
                    })
                    .map(|(source, weight)| (source as usize, weight))
                    .collect()
            } else {
                // What of the picture it covers.
                let (from, to) = (out as f64 / scale - start, (out + 1) as f64 / scale - start);
                let mut taps = Vec::new();
                let mut source = from.floor().max(0.0);
                while source < to.min(length as f64) {
                    let covered = (to.min(source + 1.0) - from.max(source)) * scale;
                    if covered > 0.0 {
                        taps.push((source as usize, covered as f32));
                    }
                    source += 1.0;
                }
                taps
            }
        })
        .collect();
    Some((first, taps))
}

/// Limited range's levels: black and luma's span, chroma's middle and
/// span, and the most a sample holds.
struct Levels {
    black: f32,
    luma: f32,
    middle: f32,
    chroma: f32,
    most: f32,
}

impl Levels {
    fn of(ten_bit: bool) -> Self {
        if ten_bit {
            Self {
                black: 64.0,
                luma: 876.0,
                middle: 512.0,
                chroma: 896.0,
                most: 1023.0,
            }
        } else {
            Self {
                black: 16.0,
                luma: 219.0,
                middle: 128.0,
                chroma: 224.0,
                most: 255.0,
            }
        }
    }
}

/// A sample of a plane: a byte, or P010's 16 bits with the value in the
/// top ten.
fn read(plane: &[u8], at: usize, ten_bit: bool) -> f32 {
    if ten_bit {
        f32::from(u16::from_le_bytes([plane[at], plane[at + 1]]) >> 6)
    } else {
        f32::from(plane[at])
    }
}

fn write(plane: &mut [u8], at: usize, ten_bit: bool, value: f32, most: f32) {
    let value = value.round().clamp(0.0, most);
    if ten_bit {
        plane[at..at + 2].copy_from_slice(&((value as u16) << 6).to_le_bytes());
    } else {
        plane[at] = value as u8;
    }
}

/// Blend `patch` into a frame's luma plane, `stride` bytes a row.
fn blend_luma(plane: &mut [u8], stride: usize, patch: &Patch, ten_bit: bool) {
    if patch.width == 0 {
        return;
    }
    let levels = Levels::of(ten_bit);
    let bytes = if ten_bit { 2 } else { 1 };
    for (row, pixels) in patch.pixels.chunks(patch.width).enumerate() {
        for (column, &[y, _, _, alpha]) in pixels.iter().enumerate() {
            let at = (patch.y + row) * stride + (patch.x + column) * bytes;
            if alpha <= 0.0 || at + bytes > plane.len() {
                continue;
            }
            let under = read(plane, at, ten_bit);
            let over = levels.black * alpha + levels.luma * y;
            write(
                plane,
                at,
                ten_bit,
                under * (1.0 - alpha) + over,
                levels.most,
            );
        }
    }
}

/// Blend `patch` into a frame's chroma plane, `stride` bytes a row: blue
/// and red differences interleaved, each at half the size each way.
fn blend_chroma(plane: &mut [u8], stride: usize, patch: &Patch, ten_bit: bool) {
    if patch.width == 0 || patch.height == 0 {
        return;
    }
    let levels = Levels::of(ten_bit);
    let bytes = if ten_bit { 2 } else { 1 };
    let pixel = |x: usize, y: usize| {
        let (column, row) = (x.checked_sub(patch.x)?, y.checked_sub(patch.y)?);
        (column < patch.width && row < patch.height)
            .then(|| patch.pixels[row * patch.width + column])
    };
    for row in patch.y / 2..=(patch.y + patch.height - 1) / 2 {
        for column in patch.x / 2..=(patch.x + patch.width - 1) / 2 {
            // The average of the four luma pixels it goes with.
            let mut sum = [0.0f32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                if let Some(pixel) = pixel(column * 2 + dx, row * 2 + dy) {
                    for (sum, value) in sum.iter_mut().zip(pixel) {
                        *sum += value / 4.0;
                    }
                }
            }
            let [_, cb, cr, alpha] = sum;
            let at = row * stride + column * 2 * bytes;
            if alpha <= 0.0 || at + 2 * bytes > plane.len() {
                continue;
            }
            for (offset, value) in [(0, cb), (bytes, cr)] {
                let under = read(plane, at + offset, ten_bit);
                let over = levels.middle * alpha + levels.chroma * value;
                write(
                    plane,
                    at + offset,
                    ten_bit,
                    under * (1.0 - alpha) + over,
                    levels.most,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDR: Coding = Coding {
        kr: 0.2126,
        kb: 0.0722,
        transfer: Transfer::Sdr,
    };

    fn bar(x: i32, y: i32, width: usize, height: usize) -> Picture {
        // White, outlined in black, on nothing.
        let indices = (0..height)
            .flat_map(|row| {
                (0..width).map(move |column| {
                    if row == 0 || column == 0 || row == height - 1 || column == width - 1 {
                        2
                    } else {
                        1
                    }
                })
            })
            .collect();
        Picture {
            x,
            y,
            width,
            height,
            indices,
            palette: vec![0x0000_0000, 0xFFFF_FFFF, 0xFF00_0000],
        }
    }

    fn sets() -> Sets {
        let mut sets = Sets::default();
        sets.add(0.2, Some(1.0), vec![bar(0, 0, 4, 4)], (8, 8));
        sets.add(1.2, None, vec![bar(4, 4, 4, 4)], (8, 8));
        sets.add(2.0, None, Vec::new(), (8, 8));
        sets
    }

    #[test]
    fn shows_each_set_from_its_start_to_its_end_or_the_next() {
        let mut sets = sets();
        let showing = |sets: &mut Sets, time| sets.at(time).map(|set| set.id);
        assert_eq!(showing(&mut sets, 0.0), None);
        assert_eq!(showing(&mut sets, 0.2), Some(1));
        assert_eq!(showing(&mut sets, 0.96), Some(1));
        assert_eq!(showing(&mut sets, 1.0), None, "it ends");
        assert_eq!(showing(&mut sets, 1.2), Some(2));
        assert_eq!(showing(&mut sets, 1.96), Some(2), "it has no end");
        assert_eq!(showing(&mut sets, 2.0), None, "an empty set clears it");
        assert_eq!(sets.sets.len(), 1, "sets replaced go");
    }

    #[test]
    fn keeps_sets_in_order_however_they_come() {
        let mut sets = Sets::default();
        sets.add(1.2, None, vec![bar(0, 0, 2, 2)], (8, 8));
        sets.add(0.2, None, vec![bar(0, 0, 2, 2)], (8, 8));
        sets.add(0.7, None, vec![bar(0, 0, 2, 2)], (8, 8));
        assert_eq!(sets.at(0.5).map(|set| set.id), Some(2));
        assert_eq!(sets.at(0.8).map(|set| set.id), Some(3));
        assert_eq!(sets.at(1.5).map(|set| set.id), Some(1));
    }

    #[test]
    fn converts_palette_colors_premultiplied() {
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-3);
        assert!(close(SDR.color(0xFFFF_FFFF), [1.0, 0.0, 0.0, 1.0]));
        assert!(close(SDR.color(0xFF00_0000), [0.0, 0.0, 0.0, 1.0]));
        assert!(close(SDR.color(0x80FF_FFFF), [0.502, 0.0, 0.0, 0.502]));
        assert!(close(SDR.color(0x0012_3456), [0.0; 4]));
        let red = SDR.color(0xFFFF_0000);
        assert!((red[0] - 0.2126).abs() < 1e-3 && (red[2] - 0.5).abs() < 1e-3);
        // HDR's white is graphics white, not the brightest.
        let pq = Coding {
            transfer: Transfer::Pq,
            ..SDR
        };
        assert!((pq.color(0xFFFF_FFFF)[0] - 0.58).abs() < 0.01);
        let hlg = Coding {
            transfer: Transfer::Hlg,
            ..SDR
        };
        assert!((hlg.color(0xFFFF_FFFF)[0] - 0.75).abs() < 0.01);
    }

    #[test]
    fn copies_a_picture_at_the_canvas_size() {
        let patch = patch(&bar(2, 1, 4, 3), (8, 8), (8, 8), &SDR).unwrap();
        // A pixel of nothing either side.
        assert_eq!((patch.x, patch.y), (1, 0));
        assert_eq!((patch.width, patch.height), (6, 5));
        let alpha: Vec<f32> = patch.pixels.iter().map(|pixel| pixel[3]).collect();
        assert_eq!(&alpha[..6], &[0.0; 6]);
        assert_eq!(&alpha[6..12], &[0.0, 1.0, 1.0, 1.0, 1.0, 0.0]);
        let luma: Vec<f32> = patch.pixels[12..18].iter().map(|pixel| pixel[0]).collect();
        assert_eq!(luma, [0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn scales_a_picture_with_its_canvas() {
        // Twice the size, interpolated.
        let big = patch(&bar(2, 2, 4, 4), (8, 8), (16, 16), &SDR).unwrap();
        let opaque = |patch: &Patch| {
            let mut found = (usize::MAX, usize::MAX, 0, 0);
            for (index, pixel) in patch.pixels.iter().enumerate() {
                let (x, y) = (patch.x + index % patch.width, patch.y + index / patch.width);
                if pixel[3] > 0.99 {
                    found = (
                        found.0.min(x),
                        found.1.min(y),
                        found.2.max(x),
                        found.3.max(y),
                    );
                }
            }
            found
        };
        // It covers 4 to 11; its edges are part covered.
        assert_eq!(opaque(&big), (5, 5, 10, 10));
        let edge = big.pixels[(8 - big.y) * big.width + (4 - big.x)];
        assert!((edge[3] - 0.75).abs() < 1e-3, "{edge:?}");
        // Half the size, averaged: its outline and middle mix to gray.
        let small = patch(&bar(2, 2, 4, 4), (8, 8), (4, 4), &SDR).unwrap();
        assert_eq!(opaque(&small), (1, 1, 2, 2));
        let inside = small.pixels[(1 - small.y) * small.width + (1 - small.x)];
        assert!((inside[0] - 0.25).abs() < 1e-3, "{inside:?}");
        // A quarter of the size: all of a corner's 4x4 pixels averaged,
        // 7 of the 16 outline.
        let quarter = patch(&bar(0, 0, 8, 8), (8, 8), (2, 2), &SDR).unwrap();
        assert!(
            (quarter.pixels[0][0] - 9.0 / 16.0).abs() < 1e-3,
            "{quarter:?}"
        );
        // Off the frame, nothing.
        assert!(patch(&bar(20, 2, 4, 4), (8, 8), (8, 8), &SDR).is_none());
    }

    #[test]
    fn takes_the_frames_colors() {
        use ffi::AVColorSpace::*;
        use ffi::AVColorTransferCharacteristic::*;
        let colors = |space, transfer| Colors {
            primaries: None,
            transfer: Some(transfer),
            space: Some(space),
        };
        let bt2020 = Coding {
            kr: 0.2627,
            kb: 0.0593,
            transfer: Transfer::Sdr,
        };
        let hdr10 = colors(AVCOL_SPC_BT2020_NCL, AVCOL_TRC_SMPTE2084);
        assert_eq!(
            Coding::of(&hdr10, true),
            Coding {
                transfer: Transfer::Pq,
                ..bt2020
            }
        );
        let hlg = colors(AVCOL_SPC_BT2020_NCL, AVCOL_TRC_ARIB_STD_B67);
        assert_eq!(
            Coding::of(&hlg, true),
            Coding {
                transfer: Transfer::Hlg,
                ..bt2020
            }
        );
        let sd = colors(AVCOL_SPC_SMPTE170M, AVCOL_TRC_BT709);
        assert_eq!(Coding::of(&sd, true).kr, 0.299);
        // 8-bit encodings are BT.709 SDR, whatever the source was.
        assert_eq!(Coding::of(&hdr10, false), SDR);
        assert_eq!(Coding::of(&Colors::default(), true), SDR);
    }

    #[test]
    fn blends_into_nv12_in_limited_range() {
        let (width, height) = (8, 4);
        let mut luma = vec![100u8; width * height];
        let mut chroma = vec![128u8; width * height / 2];
        let patch = Patch {
            x: 2,
            y: 0,
            width: 2,
            height: 2,
            pixels: vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.5, 0.0, 0.25, 0.5],
                [0.0, 0.0, 0.0, 1.0],
                [0.0; 4],
            ],
        };
        blend_luma(&mut luma, width, &patch, false);
        blend_chroma(&mut chroma, width, &patch, false);
        assert_eq!(&luma[..width], &[100, 100, 235, 168, 100, 100, 100, 100]);
        assert_eq!(
            &luma[width..2 * width],
            &[100, 100, 16, 100, 100, 100, 100, 100]
        );
        assert_eq!(&luma[2 * width..], &[100; 16]);
        // The chroma pair over x 2-3, y 0-1: 5/8 covered, red a little.
        assert_eq!(&chroma[2..4], &[128, 142]);
        assert_eq!(&chroma[..2], &[128, 128]);
    }

    #[test]
    fn blends_into_p010_in_its_top_ten_bits() {
        let mut luma = vec![0u8; 8];
        let mut chroma = vec![0u8; 8];
        for plane in [&mut luma, &mut chroma] {
            for sample in plane.chunks_mut(2) {
                sample.copy_from_slice(&(512u16 << 6).to_le_bytes());
            }
        }
        let patch = Patch {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            pixels: vec![[1.0, 0.0, 0.0, 1.0]; 4],
        };
        blend_luma(&mut luma, 4, &patch, true);
        blend_chroma(&mut chroma, 4, &patch, true);
        assert_eq!(u16::from_le_bytes([luma[0], luma[1]]), 940 << 6);
        assert_eq!(u16::from_le_bytes([luma[6], luma[7]]), 940 << 6);
        assert_eq!(u16::from_le_bytes([chroma[0], chroma[1]]), 512 << 6);
        assert_eq!(u16::from_le_bytes([chroma[4], chroma[5]]), 512 << 6);
    }
}
