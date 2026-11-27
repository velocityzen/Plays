//! HDR converted for a device that isn't given it: fitted to SDR as Plays
//! shows it on an SDR screen (`src/media/hdr.rs`, with the same constants),
//! so a cast looks as the file does there, not washed out, as PQ's or HLG's
//! codes look taken for BT.709's. Dolby Vision's IPT, which no device is
//! sent as it is, is reshaped by its RPU (`dovi`) into HDR10 for one that
//! shows HDR10, and fitted to SDR as HDR10 is for one that doesn't.
//!
//! HDR's reference white of 203 nits (ITU-R BT.2408) lands on SDR's white.
//! Everything below three quarters of it is left alone, and the rest rolls
//! off toward white, as far as the video's peak says (its MaxCLL or its
//! mastering display's), so nothing clips. Its BT.2020 colors become
//! BT.709's, and the light is coded for a TV's BT.1886 (a power of 2.4).

use std::ffi::c_uint;
use std::ptr::NonNull;

use ffmpeg::format::Pixel;
use ffmpeg::util::color;
use ffmpeg::{ffi, frame};
use ffmpeg_next as ffmpeg;
use ffmpeg_safe::{Tags, sys};
use rayon::prelude::*;

use crate::Error;
use crate::dovi::Reshaping;

/// HDR reference white (BT.2408), shown as SDR white.
const REFERENCE_WHITE_NITS: f32 = 203.0;
/// The display HLG is rendered for (BT.2100's nominal 1000 nits), which
/// sets its system gamma of 1.2.
const HLG_PEAK_NITS: f32 = 1000.0;
const HLG_GAMMA: f32 = 1.2;
/// Brightness below this share of SDR white is left alone.
const KNEE: f32 = 0.75;
/// PQ's peak, which no video is mastered over.
const PQ_PEAK_NITS: f32 = 10000.0;

/// PQ's constants (SMPTE ST 2084).
const PQ_M1: f32 = 2610.0 / 16384.0;
const PQ_M2: f32 = 2523.0 / 4096.0 * 128.0;
const PQ_C1: f32 = 3424.0 / 4096.0;
const PQ_C2: f32 = 2413.0 / 4096.0 * 32.0;
const PQ_C3: f32 = 2392.0 / 4096.0 * 32.0;
/// HLG's OETF constants (BT.2100).
const HLG_A: f32 = 0.178_832_77;
const HLG_B: f32 = 1.0 - 4.0 * HLG_A;
const HLG_C: f32 = 0.559_910_7;

const BT2020_TO_BT709: [[f32; 3]; 3] = [
    [1.660_491, -0.587_641, -0.072_850],
    [-0.124_550, 1.1329, -0.008_349],
    [-0.018_151, -0.100_579, 1.118_73],
];

/// The power a TV decodes SDR with (BT.1886).
pub(crate) const TV_GAMMA: f32 = 2.4;
/// Entries in the decoding table: the top 12 bits of each 16-bit sample,
/// more than the 10 bits HDR video carries.
const DECODE_BITS: u32 = 12;
/// Steps in the encoding table, enough that dark tones don't band.
const ENCODE_STEPS: usize = 16384;

/// How an HDR picture codes light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transfer {
    /// SMPTE ST 2084: HDR10, HDR10+ and most Dolby Vision.
    Pq,
    /// Hybrid log-gamma (ARIB STD-B67).
    Hlg,
}

impl Transfer {
    /// `frame`'s, where it's HDR in YCbCr, whatever its matrix, as the
    /// player takes it: not IPT, as Dolby Vision's profile 20 is, which
    /// only its reshaping makes HDR10.
    pub fn of(frame: &frame::Video) -> Option<Self> {
        use ffi::AVColorSpace::AVCOL_SPC_IPT_C2;
        use ffi::AVColorTransferCharacteristic::{AVCOL_TRC_ARIB_STD_B67, AVCOL_TRC_SMPTE2084};
        let Tags {
            transfer, space, ..
        } = Tags::of_frame(frame);
        if space == AVCOL_SPC_IPT_C2 as u32 {
            return None;
        }
        if transfer == AVCOL_TRC_SMPTE2084 as u32 {
            Some(Self::Pq)
        } else if transfer == AVCOL_TRC_ARIB_STD_B67 as u32 {
            Some(Self::Hlg)
        } else {
            None
        }
    }
}

/// PQ's EOTF: a signal in 0..=1 to nits.
pub(crate) fn pq_to_nits(signal: f32) -> f32 {
    let p = signal.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    10000.0 * ((p - PQ_C1).max(0.0) / (PQ_C2 - PQ_C3 * p)).powf(1.0 / PQ_M1)
}

/// PQ's inverse EOTF: nits to a signal in 0..=1.
pub(crate) fn nits_to_pq(nits: f32) -> f32 {
    let y = (nits / PQ_PEAK_NITS).clamp(0.0, 1.0).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * y) / (1.0 + PQ_C3 * y)).powf(PQ_M2)
}

/// `nits` if a video's peak could be that: from HDR's reference white to
/// PQ's peak.
pub(crate) fn plausible_peak(nits: f32) -> Option<f32> {
    (REFERENCE_WHITE_NITS..=PQ_PEAK_NITS)
        .contains(&nits)
        .then_some(nits)
}

/// HLG's inverse OETF: a signal in 0..=1 to scene light in 0..=1.
pub(crate) fn hlg_to_scene(signal: f32) -> f32 {
    let e = signal.clamp(0.0, 1.0);
    if e <= 0.5 {
        e * e / 3.0
    } else {
        (((e - HLG_C) / HLG_A).exp() + HLG_B) / 12.0
    }
}

/// HLG's OOTF: scene light in BT.2020 RGB to nits on its nominal display.
fn hlg_scene_to_nits(rgb: [f32; 3]) -> [f32; 3] {
    let luma = 0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2];
    let gain = HLG_PEAK_NITS * luma.max(0.0).powf(HLG_GAMMA - 1.0);
    rgb.map(|c| gain * c)
}

/// Brightness relative to SDR white, fitted under it: unchanged up to the
/// knee, then rolled off. With the video's `peak` (relative to SDR white
/// too) it reaches white there, and a video that fits is left alone;
/// without, it eases toward white without reaching it.
pub(crate) fn tone_curve(x: f32, peak: Option<f32>) -> f32 {
    if x <= KNEE {
        return x;
    }
    let room = 1.0 - KNEE;
    let past = (x - KNEE) / room;
    let eased = match peak {
        Some(peak) => {
            // How many rooms past the knee the peak is: one if it fits.
            let white = ((peak - KNEE) / room).max(1.0);
            (past * (1.0 + past / (white * white)) / (1.0 + past)).min(1.0)
        }
        None => 1.0 - (-past).exp(),
    };
    KNEE + room * eased
}

/// The brightest `frame`'s video gets, in nits, as far as anything says.
/// HLG is always as on its nominal display. PQ's HDR10 metadata says: the
/// brightest pixel of the whole video (MaxCLL), or the peak of the display
/// it was mastered on, whichever is lower when it gives both.
pub(crate) fn peak_nits(frame: &frame::Video, transfer: Transfer) -> Option<f32> {
    if transfer == Transfer::Hlg {
        return Some(HLG_PEAK_NITS);
    }
    let light = ffmpeg_safe::frame::<sys::ContentLightLevel>(frame);
    let display = ffmpeg_safe::frame::<sys::MasteringDisplay>(frame);
    let brightest = light.map(|light| light.max_cll as f32);
    // Over nothing, a peak is no number, which isn't plausible.
    let mastered = display
        .filter(|display| display.has_luminance != 0)
        .map(|display| display.max_luminance.num as f32 / display.max_luminance.den as f32);
    [brightest, mastered]
        .into_iter()
        .flatten()
        .filter_map(plausible_peak)
        .reduce(f32::min)
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
    tables: Tables,
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
            tables: Tables::default(),
            planes: frame::Video::empty(),
            rgb: frame::Video::empty(),
        }
    }
}

/// Light decoded and encoded by table, too slow to work out per pixel.
struct Tables {
    /// Signal to linear light: PQ's relative to SDR white, HLG's scene
    /// light (whose OOTF needs all three channels).
    pq: Vec<f32>,
    hlg: Vec<f32>,
    /// A 16-bit PQ signal to nits, for Dolby Vision's LMS.
    pq_nits: Vec<f32>,
    /// Nits to a 16-bit PQ sample, by the fourth root of their share of
    /// PQ's peak, which steps finely through the dark.
    pq_encode: Vec<u16>,
    /// Linear light in 0..=1 to a 16-bit BT.1886 sample.
    encode: Vec<u16>,
}

impl Default for Tables {
    fn default() -> Self {
        let decode = |bits: u32, to_light: &dyn Fn(f32) -> f32| {
            let max = ((1 << bits) - 1) as f32;
            (0..1 << bits)
                .map(|i| to_light(i as f32 / max))
                .collect::<Vec<_>>()
        };
        let steps = |to_sample: &dyn Fn(f32) -> f32| {
            (0..=ENCODE_STEPS)
                .map(|i| (to_sample(i as f32 / ENCODE_STEPS as f32) * 65535.0).round() as u16)
                .collect::<Vec<_>>()
        };
        Self {
            pq: decode(DECODE_BITS, &|signal| {
                pq_to_nits(signal) / REFERENCE_WHITE_NITS
            }),
            hlg: decode(DECODE_BITS, &hlg_to_scene),
            pq_nits: decode(16, &pq_to_nits),
            pq_encode: steps(&|root| nits_to_pq(root.powi(4) * PQ_PEAK_NITS)),
            encode: steps(&|light| light.powf(1.0 / TV_GAMMA)),
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
        let Tables {
            pq,
            hlg,
            pq_nits,
            pq_encode,
            encode,
        } = &self.tables;
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
                let peak = peak_nits(from, transfer).map(|nits| nits / REFERENCE_WHITE_NITS);
                let decode = match transfer {
                    Transfer::Pq => pq,
                    Transfer::Hlg => hlg,
                };
                rows(&mut self.rgb, stride, width, height, |_, pixels| {
                    for pixel in pixels {
                        let light = read(pixel)
                            .map(|sample| decode[(sample >> (16 - DECODE_BITS)) as usize]);
                        let relative = match transfer {
                            Transfer::Pq => light,
                            Transfer::Hlg => {
                                hlg_scene_to_nits(light).map(|nits| nits / REFERENCE_WHITE_NITS)
                            }
                        };
                        write(pixel, fit(relative, peak, encode));
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
                            .map(|signal| pq_nits[(signal * 65535.0 + 0.5) as usize]);
                        let nits = reshaping.lms_to_rgb(lms);
                        write(
                            pixel,
                            if hdr10 {
                                nits.map(|nits| {
                                    let root = (nits / PQ_PEAK_NITS).clamp(0.0, 1.0).sqrt().sqrt();
                                    pq_encode[(root * ENCODE_STEPS as f32 + 0.5) as usize]
                                })
                            } else {
                                fit(nits.map(|nits| nits / REFERENCE_WHITE_NITS), peak, encode)
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
/// 16-bit BT.1886 samples, by `encode`, rolled off toward the video's
/// `peak` (relative too).
fn fit(relative: [f32; 3], peak: Option<f32>, encode: &[u16]) -> [u16; 3] {
    let rgb = BT2020_TO_BT709
        .map(|row| row[0] * relative[0] + row[1] * relative[1] + row[2] * relative[2]);
    let brightest = rgb[0].max(rgb[1]).max(rgb[2]);
    let scale = if brightest > 0.0 {
        tone_curve(brightest, peak) / brightest
    } else {
        0.0
    };
    rgb.map(|c| encode[((c * scale).clamp(0.0, 1.0) * ENCODE_STEPS as f32 + 0.5) as usize])
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

/// A new reference to `frame`'s pictures, tagged as `coding`.
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
