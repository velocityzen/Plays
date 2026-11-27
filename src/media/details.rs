//! What the console shows about the streams playing: their formats as the
//! file has them, read from their decoders before they decode anything.

use std::ffi::{CStr, c_char, c_int};

use ffmpeg::util::{color, format::Pixel, format::Sample};
use ffmpeg::{codec, ffi};
use ffmpeg_next as ffmpeg;

use ffmpeg_safe::{Colors, DolbyVision};

/// The file or stream as a whole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerDetails {
    /// Its format, such as "QuickTime / MOV".
    pub format: String,
    /// Bits a second for everything in it, if known.
    pub bit_rate: Option<u64>,
}

/// The video stream playing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoDetails {
    /// The codec's profile, such as "High" or "Main 10".
    pub profile: Option<String>,
    /// The size its pictures are coded at, before any reshaping.
    pub coded_size: (u32, u32),
    /// Such as "yuv420p10le".
    pub pixel_format: Option<&'static str>,
    /// The Y'CbCr matrix, primaries and transfer, as FFmpeg names them.
    pub matrix: Option<&'static str>,
    pub primaries: Option<&'static str>,
    pub transfer: Option<&'static str>,
    /// Full range ("pc") rather than video range ("tv"), if it says.
    pub full_range: Option<bool>,
    pub bit_rate: Option<u64>,
    /// Dolby Vision, which Plays reshapes into HDR whatever its transfer
    /// says: profile 5's says nothing.
    pub dolby_vision: bool,
}

/// The audio stream playing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioDetails {
    pub profile: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
    /// Such as "5.1(side)".
    pub layout: Option<String>,
    /// Such as "fltp".
    pub sample_format: Option<&'static str>,
    /// Bits a sample, for formats that keep them: lossless ones, and PCM.
    pub bits: Option<u32>,
    pub bit_rate: Option<u64>,
}

impl ContainerDetails {
    pub(super) fn of(input: &ffmpeg::format::context::Input) -> Self {
        Self {
            format: input.format().description().to_owned(),
            bit_rate: positive(input.bit_rate()),
        }
    }
}

impl VideoDetails {
    pub(super) fn of(decoder: &ffmpeg::decoder::Video) -> Self {
        let colors = Colors::of_decoder(decoder);
        let full_range = match colors.range {
            color::Range::JPEG => Some(true),
            color::Range::MPEG => Some(false),
            color::Range::Unspecified => None,
        };
        Self {
            profile: profile(decoder),
            coded_size: (decoder.width(), decoder.height()),
            pixel_format: Some(decoder.format())
                .filter(|format| *format != Pixel::None)
                .and_then(Pixel::descriptor)
                .map(|descriptor| descriptor.name()),
            matrix: Some(colors.space)
                .filter(|space| *space != color::Space::Unspecified)
                .and_then(|space| space.name()),
            primaries: Some(colors.primaries)
                .filter(|primaries| *primaries != color::Primaries::Unspecified)
                .and_then(|primaries| primaries.name()),
            transfer: Some(colors.transfer)
                .filter(|transfer| *transfer != color::TransferCharacteristic::Unspecified)
                .and_then(|transfer| transfer.name()),
            full_range,
            bit_rate: positive(decoder.bit_rate() as i64),
            dolby_vision: DolbyVision::of(&codec::Parameters::from(decoder)).is_some(),
        }
    }

    /// Whether Plays shows it as HDR: PQ or HLG, or Dolby Vision.
    pub fn hdr(&self) -> bool {
        self.dolby_vision || matches!(self.transfer, Some("smpte2084" | "arib-std-b67"))
    }
}

impl AudioDetails {
    pub(super) fn of(decoder: &ffmpeg::decoder::Audio) -> Self {
        Self {
            profile: profile(decoder),
            sample_rate: decoder.rate(),
            channels: decoder.channels(),
            layout: layout(decoder),
            sample_format: Some(decoder.format())
                .filter(|format| *format != Sample::None)
                .map(|format| format.name()),
            bits: bits(decoder),
            bit_rate: positive(decoder.bit_rate() as i64),
        }
    }
}

/// The bits a sample its decoder says it has, or its codec has if it has
/// a fixed number; lossy codecs have none.
fn bits(decoder: &ffmpeg::decoder::Audio) -> Option<u32> {
    // SAFETY: the context is open.
    unsafe {
        let context = decoder.as_ptr();
        sample_bits((*context).bits_per_raw_sample, (*context).codec_id)
    }
}

/// The bits a sample has, `raw` as a decoder or a stream says, or as
/// `codec` has if it has a fixed number.
pub(super) fn sample_bits(raw: c_int, codec: ffi::AVCodecID) -> Option<u32> {
    let bits = match raw {
        // SAFETY: looks the codec up in FFmpeg's table.
        0 => unsafe { ffi::av_get_bits_per_sample(codec) },
        raw => raw,
    };
    u32::try_from(bits).ok().filter(|bits| *bits > 0)
}

pub(super) fn positive(bit_rate: i64) -> Option<u64> {
    u64::try_from(bit_rate).ok().filter(|rate| *rate > 0)
}

/// The profile's name, for codecs that have them.
fn profile(decoder: &ffmpeg::codec::decoder::Opened) -> Option<String> {
    // SAFETY: the context is open.
    unsafe {
        let context = decoder.as_ptr();
        profile_name((*context).codec_id, (*context).profile)
    }
}

/// The name of `codec`'s profile `profile`, for codecs that have them,
/// such as "High" or "Main 10".
pub(super) fn profile_name(codec: ffi::AVCodecID, profile: c_int) -> Option<String> {
    // SAFETY: FFmpeg's names are static strings.
    unsafe { string(ffi::avcodec_profile_name(codec, profile)) }
}

/// The channel layout's name, such as "stereo".
fn layout(decoder: &ffmpeg::decoder::Audio) -> Option<String> {
    // SAFETY: the context is open.
    unsafe { layout_name(&(*decoder.as_ptr()).ch_layout) }
}

/// `layout`'s name, such as "stereo" or "5.1(side)".
pub(super) fn layout_name(layout: &ffi::AVChannelLayout) -> Option<String> {
    let mut name = [0 as c_char; 64];
    // SAFETY: the layout is borrowed; FFmpeg writes a terminated string of
    // at most the buffer's size.
    unsafe {
        if ffi::av_channel_layout_describe(layout, name.as_mut_ptr(), name.len()) < 0 {
            return None;
        }
        string(name.as_ptr())
    }
}

/// # Safety
/// `name` is null or a terminated string.
pub(super) unsafe fn string(name: *const c_char) -> Option<String> {
    if name.is_null() {
        return None;
    }
    // SAFETY: as the caller promises.
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    (!name.is_empty()).then(|| name.into_owned())
}

#[cfg(test)]
#[path = "tests/details.rs"]
mod tests;
