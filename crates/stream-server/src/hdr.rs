//! HDR converted for a device that isn't given it: fitted to SDR as Plays
//! shows it on an SDR screen, by the `hdr` crate's tone curve, so a cast
//! looks as the file does there, not washed out, as PQ's or HLG's codes
//! look taken for BT.709's. Dolby Vision's IPT, which no device is sent as
//! it is, is reshaped by its RPU into HDR10 for one that shows HDR10, and
//! fitted to SDR as HDR10 is for one that doesn't.
//!
//! Fitted to SDR, its BT.2020 colors become BT.709's, and the light is
//! coded for a TV's BT.1886 (a power of 2.4).

use std::ffi::c_uint;
use std::ptr::NonNull;

use ::hdr::{
    REFERENCE_WHITE_NITS, Reshaping, Tables, Transfer, bt2020_to_bt709, hlg_scene_to_nits,
    peak_nits, tone_map,
};
use ffmpeg::format::Pixel;
use ffmpeg::util::color;
use ffmpeg::{ffi, frame};
use ffmpeg_next as ffmpeg;
use ffmpeg_safe::Tags;
use rayon::prelude::*;

use crate::Error;

/// The power a TV decodes SDR with (BT.1886).
pub(crate) const TV_GAMMA: f32 = 2.4;
/// Steps in the encoding table, enough that dark tones don't band.
const ENCODE_STEPS: usize = 16384;

/// `frame`'s transfer, where it's HDR in YCbCr, whatever its matrix, as
/// the player takes it: not IPT, as Dolby Vision's profile 20 is, which
/// only its reshaping makes HDR10.
pub(crate) fn transfer(frame: &frame::Video) -> Option<Transfer> {
    let ipt = Tags::of_frame(frame).space == ffi::AVColorSpace::AVCOL_SPC_IPT_C2 as u32;
    Transfer::of(frame).filter(|_| !ipt)
}

/// What a frame is converted from, and into.
#[derive(Clone, Copy)]
pub(crate) enum Conversion<'a> {
    /// HDR10's or HLG's YCbCr, fitted to SDR.
    ToSdr(Transfer),
    /// Dolby Vision's IPT, reshaped by its RPU into HDR10 (`hdr10`), or
    /// fitted to SDR.
    Reshaped {
        reshaping: &'a Reshaping,
        hdr10: bool,
    },
}

/// How a frame is coded, as its tags say to swscale.
#[derive(Clone, Copy)]
struct Coding {
    primaries: color::Primaries,
    transfer: color::TransferCharacteristic,
    space: color::Space,
    range: color::Range,
}

impl Coding {
    const SDR: Self = Self {
        primaries: color::Primaries::BT709,
        transfer: color::TransferCharacteristic::BT709,
        space: color::Space::BT709,
        range: color::Range::MPEG,
    };
    const HDR10: Self = Self {
        primaries: color::Primaries::BT2020,
        transfer: color::TransferCharacteristic::SMPTE2084,
        space: color::Space::BT2020NCL,
        range: color::Range::MPEG,
    };

    /// The same, in full-range RGB.
    fn rgb(self) -> Self {
        Self {
            space: color::Space::RGB,
            range: color::Range::JPEG,
            ..self
        }
    }

    fn tag(self, frame: &mut frame::Video) {
        frame.set_color_primaries(self.primaries);
        frame.set_color_transfer_characteristic(self.transfer);
        frame.set_color_space(self.space);
        frame.set_color_range(self.range);
    }
}

/// Converts HDR frames for a device: scaled to the size asked for, into
/// nonlinear BT.2020 RGB48 (or, Dolby Vision's IPT, 16-bit 4:4:4 samples
/// as they are), each pixel converted there into RGB48, then that into the
/// format asked for.
pub(crate) struct Converter {
    to_work: Option<Scaler>,
    from_rgb: Option<Scaler>,
    /// Linear light in 0..=1 to a 16-bit BT.1886 sample.
    bt1886: Vec<u16>,
    /// Dolby Vision's samples, reshaped into `rgb`.
    planes: frame::Video,
    rgb: frame::Video,
}

// SAFETY: the scalers are used by one thread at a time, the job's.
unsafe impl Send for Converter {}

impl Default for Converter {
    fn default() -> Self {
        Self {
            to_work: None,
            from_rgb: None,
            bt1886: (0..=ENCODE_STEPS)
                .map(|i| {
                    let light = i as f32 / ENCODE_STEPS as f32;
                    (light.powf(1.0 / TV_GAMMA) * 65535.0).round() as u16
                })
                .collect(),
            planes: frame::Video::empty(),
            rgb: frame::Video::empty(),
        }
    }
}

impl Converter {
    /// `from` converted as `conversion` says into `to`, at its size and in
    /// its format, which it's tagged in: BT.709 for SDR, BT.2020 and PQ for
    /// HDR10.
    pub fn run(
        &mut self,
        from: &frame::Video,
        conversion: Conversion,
        to: &mut frame::Video,
    ) -> Result<(), Error> {
        let (width, height) = (to.width(), to.height());
        let range = if Tags::of_frame(from).range == ffi::AVColorRange::AVCOL_RANGE_JPEG as u32 {
            color::Range::JPEG
        } else {
            color::Range::MPEG
        };
        if (self.rgb.width(), self.rgb.height()) != (width, height) {
            self.rgb = frame::Video::new(Pixel::RGB48LE, width, height);
        }
        let stride = ffmpeg_safe::plane(&self.rgb, 0)
            .expect("a frame made here, stored top down")
            .1;
        let (tables, bt1886) = (Tables::get(), &self.bt1886[..]);
        let out = match conversion {
            Conversion::ToSdr(transfer) => {
                // From its matrix, in the range the frame says, to RGB in
                // the same light, in BT.2020's primaries.
                let coding = Coding {
                    primaries: color::Primaries::BT2020,
                    transfer: match transfer {
                        Transfer::Pq => color::TransferCharacteristic::SMPTE2084,
                        Transfer::Hlg => color::TransferCharacteristic::ARIB_STD_B67,
                    },
                    space: matrix(from),
                    range,
                };
                coding.rgb().tag(&mut self.rgb);
                scaler(&mut self.to_work)?.run(&retagged(from, coding)?, &mut self.rgb)?;
                let peak = peak_nits(from).map(|nits| nits / REFERENCE_WHITE_NITS);
                rows(&mut self.rgb, stride, width, height, |_, pixels| {
                    for pixel in pixels {
                        let signal = read(pixel).map(|sample| f32::from(sample) / 65535.0);
                        let nits = match transfer {
                            Transfer::Pq => signal.map(|signal| tables.pq_nits(signal)),
                            Transfer::Hlg => {
                                hlg_scene_to_nits(signal.map(|signal| tables.hlg_scene(signal)))
                            }
                        };
                        write(
                            pixel,
                            fit(nits.map(|nits| nits / REFERENCE_WHITE_NITS), peak, bt1886),
                        );
                    }
                });
                Coding::SDR
            }
            Conversion::Reshaped { reshaping, hdr10 } => {
                // Its samples aren't YCbCr: they're only scaled, never
                // converted, tagged alike on either side.
                let coding = Coding {
                    range,
                    ..Coding::HDR10
                };
                if (self.planes.width(), self.planes.height()) != (width, height) {
                    self.planes = frame::Video::new(Pixel::YUV444P16LE, width, height);
                }
                coding.tag(&mut self.planes);
                scaler(&mut self.to_work)?.run(&retagged(from, coding)?, &mut self.planes)?;
                let planes: [(&[u8], usize); 3] = std::array::from_fn(|i| {
                    ffmpeg_safe::plane(&self.planes, i).expect("a frame made here, stored top down")
                });
                let peak = reshaping
                    .peak_nits()
                    .map(|nits| nits / REFERENCE_WHITE_NITS);
                rows(&mut self.rgb, stride, width, height, |y, pixels| {
                    let lines =
                        planes.map(|(data, stride)| &data[y * stride..][..pixels.len() * 2]);
                    for (x, pixel) in pixels.iter_mut().enumerate() {
                        // 16-bit samples hold the base layer's codes in their
                        // top bits.
                        let samples = lines.map(|line| {
                            f32::from(u16::from_le_bytes([line[2 * x], line[2 * x + 1]])) / 65536.0
                        });
                        let lms = reshaping
                            .lms_signal(samples)
                            .map(|signal| tables.pq_nits(signal));
                        let nits = reshaping.lms_to_rgb(lms);
                        write(
                            pixel,
                            if hdr10 {
                                nits.map(|nits| (tables.pq_signal(nits) * 65535.0 + 0.5) as u16)
                            } else {
                                fit(nits.map(|nits| nits / REFERENCE_WHITE_NITS), peak, bt1886)
                            },
                        );
                    }
                });
                if hdr10 { Coding::HDR10 } else { Coding::SDR }
            }
        };

        // Into the matrix it's tagged with, in limited range.
        out.rgb().tag(&mut self.rgb);
        out.tag(to);
        scaler(&mut self.from_rgb)?.run(&self.rgb, to)
    }
}

/// Linear BT.2020 light, relative to SDR white, fitted to SDR: BT.709's
/// 16-bit BT.1886 samples, by `bt1886`, rolled off toward the video's
/// `peak` (relative too).
fn fit(relative: [f32; 3], peak: Option<f32>, bt1886: &[u16]) -> [u16; 3] {
    tone_map(bt2020_to_bt709(relative), 1.0, peak)
        .map(|c| bt1886[(c.clamp(0.0, 1.0) * ENCODE_STEPS as f32 + 0.5) as usize])
}

/// An RGB48 pixel's samples.
fn read(pixel: &[u8; 6]) -> [u16; 3] {
    std::array::from_fn(|i| u16::from_le_bytes([pixel[2 * i], pixel[2 * i + 1]]))
}

fn write(pixel: &mut [u8; 6], samples: [u16; 3]) {
    for (i, sample) in samples.into_iter().enumerate() {
        pixel[2 * i..2 * i + 2].copy_from_slice(&sample.to_le_bytes());
    }
}

/// `convert` run on each of an RGB48 frame's rows, in parallel, with its
/// number and its pixels.
fn rows(
    rgb: &mut frame::Video,
    stride: usize,
    width: u32,
    height: u32,
    convert: impl Fn(usize, &mut [[u8; 6]]) + Sync,
) {
    ffmpeg_safe::plane_mut(rgb, 0)
        .expect("a frame made here, stored top down")
        .0
        .par_chunks_mut(stride)
        .take(height as usize)
        .enumerate()
        .for_each(|(y, row)| convert(y, row[..width as usize * 6].as_chunks_mut::<6>().0));
}

/// The scaler in `slot`, made if it isn't yet.
fn scaler(slot: &mut Option<Scaler>) -> Result<&mut Scaler, Error> {
    if slot.is_none() {
        *slot = Some(Scaler::new()?);
    }
    Ok(slot.as_mut().expect("the scaler was just made"))
}

/// FFmpeg's scaling of whole frames, slice-threaded, set up anew whenever
/// the frames it's given change. How each is tagged says how it's coded:
/// frames alike in primaries and transfer are only scaled, and their
/// matrix and range converted.
struct Scaler(NonNull<ffi::SwsContext>);

impl Scaler {
    fn new() -> Result<Self, Error> {
        // SAFETY: sws_alloc_context has no preconditions, and a context it
        // gives is ours to set before it's first used.
        unsafe {
            let context = NonNull::new(ffi::sws_alloc_context())
                .ok_or_else(|| Error::Encode("converting HDR: no scaler".into()))?;
            (*context.as_ptr()).flags = ffi::SwsFlags::SWS_BILINEAR as c_uint;
            // As many threads as there are cores.
            (*context.as_ptr()).threads = 0;
            Ok(Self(context))
        }
    }

    /// `from` scaled into `to`, whose pictures are allocated, each at the
    /// size, in the format and the colors it's tagged with.
    fn run(&mut self, from: &frame::Video, to: &mut frame::Video) -> Result<(), Error> {
        // SAFETY: the context is ours and both frames valid; FFmpeg checks
        // that one converts to the other, and writes within `to`'s
        // pictures.
        let result =
            unsafe { ffi::sws_scale_frame(self.0.as_ptr(), to.as_mut_ptr(), from.as_ptr()) };
        if result < 0 {
            return Err(Error::Encode(format!(
                "converting HDR: {}",
                ffmpeg::Error::from(result)
            )));
        }
        Ok(())
    }
}

impl Drop for Scaler {
    fn drop(&mut self) {
        let mut context = self.0.as_ptr();
        // SAFETY: the context is ours, and freed only here.
        unsafe { ffi::sws_free_context(&mut context) };
    }
}

/// The matrix HDR `frame`'s YCbCr is decoded by: its own, as the player
/// takes it, else BT.2020's, as HDR10's and HLG's is, where it says none or
/// one swscale doesn't decode (BT.2020's constant luminance, whose weights
/// are the same).
fn matrix(frame: &frame::Video) -> color::Space {
    use ffi::AVColorSpace::*;
    let space = Tags::of_frame(frame).space;
    [
        AVCOL_SPC_BT709,
        AVCOL_SPC_FCC,
        AVCOL_SPC_BT470BG,
        AVCOL_SPC_SMPTE170M,
        AVCOL_SPC_SMPTE240M,
    ]
    .into_iter()
    .find(|known| *known as u32 == space)
    .map_or(color::Space::BT2020NCL, color::Space::from)
}

/// A new reference to `frame`'s pictures, tagged as `coding`.
fn retagged(frame: &frame::Video, coding: Coding) -> Result<frame::Video, Error> {
    let mut retagged = frame::Video::empty();
    // SAFETY: both frames are valid, and the new one empty; it refers to
    // the same pictures, which FFmpeg counts references to.
    let result = unsafe { ffi::av_frame_ref(retagged.as_mut_ptr(), frame.as_ptr()) };
    if result < 0 {
        return Err(Error::Encode(format!(
            "converting HDR: {}",
            ffmpeg::Error::from(result)
        )));
    }
    coding.tag(&mut retagged);
    Ok(retagged)
}

#[cfg(test)]
#[path = "tests/hdr.rs"]
pub(crate) mod tests;
