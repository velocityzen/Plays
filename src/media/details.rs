//! What the console shows about the streams playing: their formats as the
//! file has them, read from their decoders before they decode anything.

use std::ffi::{CStr, c_char};

use ffmpeg::ffi;
use ffmpeg::util::{color, format::Pixel, format::Sample};
use ffmpeg_next as ffmpeg;

use super::colors::Colors;

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
        }
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
            bit_rate: positive(decoder.bit_rate() as i64),
        }
    }
}

fn positive(bit_rate: i64) -> Option<u64> {
    u64::try_from(bit_rate).ok().filter(|rate| *rate > 0)
}

/// The profile's name, for codecs that have them.
fn profile(decoder: &ffmpeg::codec::decoder::Opened) -> Option<String> {
    // SAFETY: the context is open, and FFmpeg's names are static strings.
    unsafe {
        let context = decoder.as_ptr();
        string(ffi::avcodec_profile_name(
            (*context).codec_id,
            (*context).profile,
        ))
    }
}

/// The channel layout's name, such as "stereo".
fn layout(decoder: &ffmpeg::decoder::Audio) -> Option<String> {
    let mut name = [0 as c_char; 64];
    // SAFETY: the context is open; FFmpeg writes a terminated string of at
    // most the buffer's size.
    unsafe {
        let context = decoder.as_ptr();
        if ffi::av_channel_layout_describe(&(*context).ch_layout, name.as_mut_ptr(), name.len()) < 0
        {
            return None;
        }
        string(name.as_ptr())
    }
}

/// # Safety
/// `name` is null or a terminated string.
unsafe fn string(name: *const c_char) -> Option<String> {
    if name.is_null() {
        return None;
    }
    // SAFETY: as the caller promises.
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    (!name.is_empty()).then(|| name.into_owned())
}
