//! A picture's colors, as its decoder, stream and frames carry them.
//! ffmpeg-next reads them as the Rust enums bound to FFmpeg's, which have
//! gaps, but a file can put any number there: Matroska's demuxer passes its
//! Colour element's on unchecked, to the decoder and every frame. As one of
//! those enums, a number no variant has is undefined behavior (a debug
//! build traps), so they're read here as the C ints they are (`Tags`), and
//! a number FFmpeg has no name for is taken to be unspecified (`Colors`).

use ffmpeg_next::util::color;
use ffmpeg_next::{codec, decoder, ffi, frame};

/// How a picture's colors are tagged, as the numbers FFmpeg keeps them as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tags {
    pub range: u32,
    pub space: u32,
    pub primaries: u32,
    pub transfer: u32,
}

impl Tags {
    pub fn of_frame(frame: &frame::Video) -> Self {
        // SAFETY: the frame is valid while borrowed, and its fields are read
        // as the C ints they are, not as the Rust enums bound to them.
        unsafe {
            let raw = frame.as_ptr();
            Self {
                range: (&raw const (*raw).color_range).cast::<u32>().read(),
                space: (&raw const (*raw).colorspace).cast::<u32>().read(),
                primaries: (&raw const (*raw).color_primaries).cast::<u32>().read(),
                transfer: (&raw const (*raw).color_trc).cast::<u32>().read(),
            }
        }
    }

    pub fn of_decoder(decoder: &decoder::Video) -> Self {
        // SAFETY: as for a frame, with the decoder's context.
        unsafe {
            let raw = decoder.as_ptr();
            Self {
                range: (&raw const (*raw).color_range).cast::<u32>().read(),
                space: (&raw const (*raw).colorspace).cast::<u32>().read(),
                primaries: (&raw const (*raw).color_primaries).cast::<u32>().read(),
                transfer: (&raw const (*raw).color_trc).cast::<u32>().read(),
            }
        }
    }

    /// As a stream's parameters say, before anything is decoded.
    pub fn of_parameters(parameters: &codec::Parameters) -> Self {
        // SAFETY: as for a frame, with the stream's parameters.
        unsafe {
            let raw = parameters.as_ptr();
            Self {
                range: (&raw const (*raw).color_range).cast::<u32>().read(),
                space: (&raw const (*raw).color_space).cast::<u32>().read(),
                primaries: (&raw const (*raw).color_primaries).cast::<u32>().read(),
                transfer: (&raw const (*raw).color_trc).cast::<u32>().read(),
            }
        }
    }

    /// The range, if one FFmpeg has a name for other than unspecified.
    pub fn range(&self) -> Option<ffi::AVColorRange> {
        RANGES.into_iter().find(|&named| named as u32 == self.range)
    }

    /// The matrix, as `range` reads it.
    pub fn space(&self) -> Option<ffi::AVColorSpace> {
        SPACES.into_iter().find(|&named| named as u32 == self.space)
    }

    /// The primaries, as `range` reads them.
    pub fn primaries(&self) -> Option<ffi::AVColorPrimaries> {
        PRIMARIES
            .into_iter()
            .find(|&named| named as u32 == self.primaries)
    }

    /// The transfer, as `range` reads it.
    pub fn transfer(&self) -> Option<ffi::AVColorTransferCharacteristic> {
        TRANSFERS
            .into_iter()
            .find(|&named| named as u32 == self.transfer)
    }

    /// As ffmpeg-next's enums, unspecified where FFmpeg has no name.
    pub fn named(&self) -> Colors {
        Colors {
            range: self.range().map_or(color::Range::Unspecified, Into::into),
            space: self.space().map_or(color::Space::Unspecified, Into::into),
            primaries: self
                .primaries()
                .map_or(color::Primaries::Unspecified, Into::into),
            transfer: self
                .transfer()
                .map_or(color::TransferCharacteristic::Unspecified, Into::into),
        }
    }
}

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
        Tags::of_frame(frame).named()
    }

    pub fn of_decoder(decoder: &decoder::Video) -> Self {
        Tags::of_decoder(decoder).named()
    }

    /// As a stream's parameters say, before anything is decoded.
    pub fn of_parameters(parameters: &codec::Parameters) -> Self {
        Tags::of_parameters(parameters).named()
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
