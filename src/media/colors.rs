//! A video's colors, as its decoder and frames carry them. ffmpeg-next reads
//! them as the Rust enums bound to FFmpeg's, which have gaps, but a file can
//! put any number there: Matroska's demuxer passes its Colour element's on
//! unchecked, to the decoder and every frame. As one of those enums, a
//! number no variant has is undefined behavior (a debug build traps), so
//! they're read here as the C ints they are, and a number FFmpeg has no name
//! for is taken to be unspecified.

use ffmpeg::util::color;
use ffmpeg::{codec, decoder, ffi, frame};
use ffmpeg_next as ffmpeg;

/// How a picture's colors are coded. Each is unspecified where the file
/// doesn't say, or says something FFmpeg has no name for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub range: color::Range,
    pub space: color::Space,
    pub primaries: color::Primaries,
    pub transfer: color::TransferCharacteristic,
}

impl Colors {
    pub fn of_frame(frame: &frame::Video) -> Self {
        // SAFETY: the frame is valid while borrowed, and its fields are read
        // as the C ints they are, not as the Rust enums bound to them.
        unsafe {
            let raw = frame.as_ptr();
            Self::named(
                (&raw const (*raw).color_range).cast::<u32>().read(),
                (&raw const (*raw).colorspace).cast::<u32>().read(),
                (&raw const (*raw).color_primaries).cast::<u32>().read(),
                (&raw const (*raw).color_trc).cast::<u32>().read(),
            )
        }
    }

    pub fn of_decoder(decoder: &decoder::Video) -> Self {
        // SAFETY: as for a frame, with the decoder's context.
        unsafe {
            let raw = decoder.as_ptr();
            Self::named(
                (&raw const (*raw).color_range).cast::<u32>().read(),
                (&raw const (*raw).colorspace).cast::<u32>().read(),
                (&raw const (*raw).color_primaries).cast::<u32>().read(),
                (&raw const (*raw).color_trc).cast::<u32>().read(),
            )
        }
    }

    /// As a stream's parameters say, before anything is decoded.
    pub fn of_parameters(parameters: &codec::Parameters) -> Self {
        // SAFETY: as for a frame, with the stream's parameters.
        unsafe {
            let raw = parameters.as_ptr();
            Self::named(
                (&raw const (*raw).color_range).cast::<u32>().read(),
                (&raw const (*raw).color_space).cast::<u32>().read(),
                (&raw const (*raw).color_primaries).cast::<u32>().read(),
                (&raw const (*raw).color_trc).cast::<u32>().read(),
            )
        }
    }

    fn named(range: u32, space: u32, primaries: u32, transfer: u32) -> Self {
        Self {
            range: RANGES
                .into_iter()
                .find(|&named| named as u32 == range)
                .map_or(color::Range::Unspecified, Into::into),
            space: SPACES
                .into_iter()
                .find(|&named| named as u32 == space)
                .map_or(color::Space::Unspecified, Into::into),
            primaries: PRIMARIES
                .into_iter()
                .find(|&named| named as u32 == primaries)
                .map_or(color::Primaries::Unspecified, Into::into),
            transfer: TRANSFERS
                .into_iter()
                .find(|&named| named as u32 == transfer)
                .map_or(color::TransferCharacteristic::Unspecified, Into::into),
        }
    }
}

const RANGES: [ffi::AVColorRange; 2] = [
    ffi::AVColorRange::AVCOL_RANGE_MPEG,
    ffi::AVColorRange::AVCOL_RANGE_JPEG,
];

const SPACES: [ffi::AVColorSpace; 16] = {
    use ffi::AVColorSpace::*;
    [
        AVCOL_SPC_RGB,
        AVCOL_SPC_BT709,
        AVCOL_SPC_FCC,
        AVCOL_SPC_BT470BG,
        AVCOL_SPC_SMPTE170M,
        AVCOL_SPC_SMPTE240M,
        AVCOL_SPC_YCGCO,
        AVCOL_SPC_BT2020_NCL,
        AVCOL_SPC_BT2020_CL,
        AVCOL_SPC_SMPTE2085,
        AVCOL_SPC_CHROMA_DERIVED_NCL,
        AVCOL_SPC_CHROMA_DERIVED_CL,
        AVCOL_SPC_ICTCP,
        AVCOL_SPC_IPT_C2,
        AVCOL_SPC_YCGCO_RE,
        AVCOL_SPC_YCGCO_RO,
    ]
};

const PRIMARIES: [ffi::AVColorPrimaries; 11] = {
    use ffi::AVColorPrimaries::*;
    [
        AVCOL_PRI_BT709,
        AVCOL_PRI_BT470M,
        AVCOL_PRI_BT470BG,
        AVCOL_PRI_SMPTE170M,
        AVCOL_PRI_SMPTE240M,
        AVCOL_PRI_FILM,
        AVCOL_PRI_BT2020,
        AVCOL_PRI_SMPTE428,
        AVCOL_PRI_SMPTE431,
        AVCOL_PRI_SMPTE432,
        AVCOL_PRI_EBU3213,
    ]
};

const TRANSFERS: [ffi::AVColorTransferCharacteristic; 16] = {
    use ffi::AVColorTransferCharacteristic::*;
    [
        AVCOL_TRC_BT709,
        AVCOL_TRC_GAMMA22,
        AVCOL_TRC_GAMMA28,
        AVCOL_TRC_SMPTE170M,
        AVCOL_TRC_SMPTE240M,
        AVCOL_TRC_LINEAR,
        AVCOL_TRC_LOG,
        AVCOL_TRC_LOG_SQRT,
        AVCOL_TRC_IEC61966_2_4,
        AVCOL_TRC_BT1361_ECG,
        AVCOL_TRC_IEC61966_2_1,
        AVCOL_TRC_BT2020_10,
        AVCOL_TRC_BT2020_12,
        AVCOL_TRC_SMPTE2084,
        AVCOL_TRC_SMPTE428,
        AVCOL_TRC_ARIB_STD_B67,
    ]
};

#[cfg(test)]
#[path = "tests/colors.rs"]
mod tests;
