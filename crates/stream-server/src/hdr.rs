//! HDR fitted to SDR, for a device that isn't given it: as Plays shows it
//! on an SDR screen (`src/media/hdr.rs`, with the same constants), so a
//! cast looks as the file does there, not washed out, as PQ's or HLG's
//! codes look taken for BT.709's.
//!
//! HDR's reference white of 203 nits (ITU-R BT.2408) lands on SDR's white.
//! Everything below three quarters of it is left alone, and the rest rolls
//! off toward white, as far as the video's peak says (its MaxCLL or its
//! mastering display's), so nothing clips. Its BT.2020 colors become
//! BT.709's, and the light is coded for a TV's BT.1886 (a power of 2.4).

use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;
use ffmpeg::util::color;
use ffmpeg::{ffi, frame};
use ffmpeg_next as ffmpeg;
use rayon::prelude::*;

use crate::Error;

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
const TV_GAMMA: f32 = 2.4;
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
    /// `frame`'s, where it's HDR in YCbCr this fits: BT.2020's (or said
    /// nothing of), not IPT, as Dolby Vision's profile 20 is, which only
    /// its reshaping makes HDR10.
    pub fn of(frame: &frame::Video) -> Option<Self> {
        use ffi::AVColorSpace::{AVCOL_SPC_BT2020_NCL, AVCOL_SPC_UNSPECIFIED};
        use ffi::AVColorTransferCharacteristic::{AVCOL_TRC_ARIB_STD_B67, AVCOL_TRC_SMPTE2084};
        let Tags {
            transfer, space, ..
        } = Tags::of(frame);
        if space != AVCOL_SPC_BT2020_NCL as u32 && space != AVCOL_SPC_UNSPECIFIED as u32 {
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

/// How a frame's colors are tagged, as the C ints FFmpeg keeps them in:
/// a file can hold numbers the Rust enums have no variant for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Tags {
    pub primaries: u32,
    pub transfer: u32,
    pub space: u32,
    pub range: u32,
}

impl Tags {
    pub fn of(frame: &frame::Video) -> Self {
        // SAFETY: the frame is valid while borrowed, and the fields are
        // read through the pointer as the C ints they are.
        unsafe {
            let raw = frame.as_ptr();
            Self {
                primaries: (&raw const (*raw).color_primaries).cast::<u32>().read(),
                transfer: (&raw const (*raw).color_trc).cast::<u32>().read(),
                space: (&raw const (*raw).colorspace).cast::<u32>().read(),
                range: (&raw const (*raw).color_range).cast::<u32>().read(),
            }
        }
    }
}

/// PQ's EOTF: a signal in 0..=1 to nits.
pub(crate) fn pq_to_nits(signal: f32) -> f32 {
    let p = signal.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    10000.0 * ((p - PQ_C1).max(0.0) / (PQ_C2 - PQ_C3 * p)).powf(1.0 / PQ_M1)
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

/// FFmpeg's structs in libavutil/mastering_display_metadata.h, which the
/// bindings don't cover.
#[allow(dead_code)]
mod sys {
    use std::ffi::{c_int, c_uint};

    use ffmpeg_next::ffi::AVRational;

    #[repr(C)]
    pub struct MasteringDisplay {
        pub display_primaries: [[AVRational; 2]; 3],
        pub white_point: [AVRational; 2],
        pub min_luminance: AVRational,
        pub max_luminance: AVRational,
        pub has_primaries: c_int,
        pub has_luminance: c_int,
    }

    #[repr(C)]
    pub struct ContentLightLevel {
        pub max_cll: c_uint,
        pub max_fall: c_uint,
    }
}

/// The brightest `frame`'s video gets, in nits, as far as anything says.
/// HLG is always as on its nominal display. PQ's HDR10 metadata says: the
/// brightest pixel of the whole video (MaxCLL), or the peak of the display
/// it was mastered on, whichever is lower when it gives both.
pub(crate) fn peak_nits(frame: &frame::Video, transfer: Transfer) -> Option<f32> {
    if transfer == Transfer::Hlg {
        return Some(HLG_PEAK_NITS);
    }
    // SAFETY: FFmpeg keeps these kinds of side data as these structs.
    let (light, display) = unsafe {
        (
            side_data::<sys::ContentLightLevel>(
                frame,
                ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL,
            ),
            side_data::<sys::MasteringDisplay>(
                frame,
                ffi::AVFrameSideDataType::AV_FRAME_DATA_MASTERING_DISPLAY_METADATA,
            ),
        )
    };
    let brightest = light.map(|light| light.max_cll as f32);
    // Over nothing, a peak is no number, which isn't plausible.
    let mastered = display
        .filter(|display| display.has_luminance != 0)
        .map(|display| display.max_luminance.num as f32 / display.max_luminance.den as f32);
    [brightest, mastered]
        .into_iter()
        .flatten()
        .filter(|nits| (REFERENCE_WHITE_NITS..=PQ_PEAK_NITS).contains(nits))
        .reduce(f32::min)
}

/// The frame's side data of `kind`, if it has some as big as a `T`.
///
/// # Safety
///
/// FFmpeg must keep that kind of side data as a `T`.
unsafe fn side_data<T>(frame: &frame::Video, kind: ffi::AVFrameSideDataType) -> Option<&T> {
    // SAFETY: side data stays valid as long as the frame, and FFmpeg
    // allocates it aligned for any struct.
    unsafe {
        let side = ffi::av_frame_get_side_data(frame.as_ptr(), kind);
        if side.is_null() || (*side).data.is_null() || ((*side).size as usize) < size_of::<T>() {
            return None;
        }
        Some(&*(*side).data.cast::<T>())
    }
}

/// A scaler, and the format and size it scales from and to.
type Scaler = (scaling::Context, (Pixel, u32, u32, Pixel, u32, u32));

/// Fits HDR frames to SDR in BT.709: scaled into nonlinear BT.2020 RGB48
/// at the size asked for, each pixel fitted there, then into the format
/// asked for.
pub(crate) struct ToSdr {
    to_rgb: Option<Scaler>,
    from_rgb: Option<Scaler>,
    rgb: frame::Video,
    /// Signal to linear light: PQ's relative to SDR white, HLG's scene
    /// light (whose OOTF needs all three channels).
    pq: Vec<f32>,
    hlg: Vec<f32>,
    /// Linear light in 0..=1 to a 16-bit BT.1886 sample.
    encode: Vec<u16>,
}

// SAFETY: the scalers are used by one thread at a time, the job's.
unsafe impl Send for ToSdr {}

impl Default for ToSdr {
    fn default() -> Self {
        let decode = |to_light: fn(f32) -> f32| {
            let max = ((1 << DECODE_BITS) - 1) as f32;
            (0..1 << DECODE_BITS)
                .map(|i| to_light(i as f32 / max))
                .collect()
        };
        Self {
            to_rgb: None,
            from_rgb: None,
            rgb: frame::Video::empty(),
            pq: decode(|signal| pq_to_nits(signal) / REFERENCE_WHITE_NITS),
            hlg: decode(hlg_to_scene),
            encode: (0..=ENCODE_STEPS)
                .map(|i| {
                    let light = i as f32 / ENCODE_STEPS as f32;
                    (light.powf(1.0 / TV_GAMMA) * 65535.0).round() as u16
                })
                .collect(),
        }
    }
}

impl ToSdr {
    /// `from`, HDR in `transfer`, fitted to SDR in `to`, at its size and in
    /// its format, which it's tagged BT.709 in.
    pub fn run(
        &mut self,
        from: &frame::Video,
        transfer: Transfer,
        to: &mut frame::Video,
    ) -> Result<(), Error> {
        let (width, height) = (to.width(), to.height());
        let to_rgb = scaler(
            &mut self.to_rgb,
            (from.format(), from.width(), from.height()),
            (Pixel::RGB48LE, width, height),
            // From BT.2020's matrix, in the range the frame says.
            ffi::SWS_CS_BT2020,
            (
                Tags::of(from).range == ffi::AVColorRange::AVCOL_RANGE_JPEG as u32,
                true,
            ),
        )?;
        if (self.rgb.width(), self.rgb.height()) != (width, height) {
            self.rgb = frame::Video::empty();
        }
        to_rgb
            .run(from, &mut self.rgb)
            .map_err(|err| Error::Encode(format!("scaling HDR: {err}")))?;

        let peak = peak_nits(from, transfer).map(|nits| nits / REFERENCE_WHITE_NITS);
        let stride = self.rgb.stride(0);
        let (decode, encode) = (
            match transfer {
                Transfer::Pq => &self.pq,
                Transfer::Hlg => &self.hlg,
            },
            &self.encode,
        );
        let fit = |pixel: &mut [u8; 6]| {
            let sample = |i: usize| u16::from_le_bytes([pixel[2 * i], pixel[2 * i + 1]]);
            let light = [0, 1, 2].map(|i| decode[(sample(i) >> (16 - DECODE_BITS)) as usize]);
            let relative = match transfer {
                Transfer::Pq => light,
                Transfer::Hlg => hlg_scene_to_nits(light).map(|nits| nits / REFERENCE_WHITE_NITS),
            };
            let rgb = BT2020_TO_BT709
                .map(|row| row[0] * relative[0] + row[1] * relative[1] + row[2] * relative[2]);
            let brightest = rgb[0].max(rgb[1]).max(rgb[2]);
            let scale = if brightest > 0.0 {
                tone_curve(brightest, peak) / brightest
            } else {
                0.0
            };
            for (i, c) in rgb.into_iter().enumerate() {
                let at = ((c * scale).clamp(0.0, 1.0) * ENCODE_STEPS as f32 + 0.5) as usize;
                pixel[2 * i..2 * i + 2].copy_from_slice(&encode[at].to_le_bytes());
            }
        };
        self.rgb
            .data_mut(0)
            .par_chunks_mut(stride)
            .take(height as usize)
            .for_each(|row| {
                row[..width as usize * 6]
                    .as_chunks_mut::<6>()
                    .0
                    .iter_mut()
                    .for_each(fit);
            });

        let from_rgb = scaler(
            &mut self.from_rgb,
            (Pixel::RGB48LE, width, height),
            (to.format(), width, height),
            // Into BT.709's matrix, in limited range.
            ffi::SWS_CS_ITU709,
            (true, false),
        )?;
        from_rgb
            .run(&self.rgb, to)
            .map_err(|err| Error::Encode(format!("scaling HDR: {err}")))?;
        to.set_color_primaries(color::Primaries::BT709);
        to.set_color_transfer_characteristic(color::TransferCharacteristic::BT709);
        to.set_color_space(color::Space::BT709);
        to.set_color_range(color::Range::MPEG);
        Ok(())
    }
}

/// `scaler`, made anew unless it scales `from` to `to` already, with YCbCr
/// in `matrix` (an `SWS_CS_*`), and whether the input and the output are in
/// full range.
fn scaler(
    scaler: &mut Option<Scaler>,
    from: (Pixel, u32, u32),
    to: (Pixel, u32, u32),
    matrix: std::ffi::c_int,
    (from_full, to_full): (bool, bool),
) -> Result<&mut scaling::Context, Error> {
    let key = (from.0, from.1, from.2, to.0, to.1, to.2);
    if scaler.as_ref().is_none_or(|(_, made)| *made != key) {
        let mut context = scaling::Context::get(
            from.0,
            from.1,
            from.2,
            to.0,
            to.1,
            to.2,
            scaling::Flags::BILINEAR,
        )
        .map_err(|err| Error::Encode(format!("scaling HDR: {err}")))?;
        // SAFETY: the context is valid while borrowed, and
        // sws_getCoefficients gives pointers to static tables.
        unsafe {
            let coefficients = ffi::sws_getCoefficients(matrix);
            ffi::sws_setColorspaceDetails(
                context.as_mut_ptr(),
                coefficients,
                i32::from(from_full),
                coefficients,
                i32::from(to_full),
                0,
                1 << 16,
                1 << 16,
            );
        }
        *scaler = Some((context, key));
    }
    Ok(&mut scaler.as_mut().expect("the scaler was just made").0)
}

#[cfg(test)]
#[path = "tests/hdr.rs"]
pub(crate) mod tests;
