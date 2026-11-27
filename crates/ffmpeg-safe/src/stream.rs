//! What a stream is, as FFmpeg has it: its codec as FFmpeg's table
//! describes it, how often its frames come, its language, and its pixels'
//! shape.

use std::ffi::{CStr, CString};

use ffmpeg_next::format::stream::Stream;
use ffmpeg_next::{DictionaryRef, Rational, codec, ffi};

use crate::tag;

/// What FFmpeg's table of codecs says of one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Described {
    /// Its long name, such as "H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10".
    pub long_name: Option<String>,
    /// Subtitles as text rather than pictures.
    pub text: bool,
}

impl Described {
    /// What the table says of `id`.
    pub fn of(id: codec::Id) -> Self {
        // SAFETY: FFmpeg's table lives as long as the program.
        unsafe { Self::of_descriptor(ffi::avcodec_descriptor_get(id.into())) }
    }

    /// What the table says of the codec FFmpeg names `name`, such as
    /// `subrip`; nothing of a name it has none by.
    pub fn named(name: &str) -> Self {
        let Ok(name) = CString::new(name) else {
            return Self::default();
        };
        // SAFETY: as above; the name is a terminated string.
        unsafe { Self::of_descriptor(ffi::avcodec_descriptor_get_by_name(name.as_ptr())) }
    }

    /// # Safety
    ///
    /// `descriptor` is null or one of FFmpeg's table's.
    unsafe fn of_descriptor(descriptor: *const ffi::AVCodecDescriptor) -> Self {
        if descriptor.is_null() {
            return Self::default();
        }
        // SAFETY: as the caller says; its long name is null or a terminated
        // string the table keeps.
        unsafe {
            let long_name = (*descriptor).long_name;
            Self {
                long_name: (!long_name.is_null())
                    .then(|| CStr::from_ptr(long_name).to_string_lossy().into_owned())
                    .filter(|name| !name.is_empty()),
                text: (*descriptor).props & ffi::AV_CODEC_PROP_TEXT_SUB != 0,
            }
        }
    }
}

/// How often `stream`'s frames come: their average, else the rate FFmpeg
/// guesses, from 1 to 480 a second.
pub fn frame_rate(stream: &Stream) -> Option<Rational> {
    [stream.avg_frame_rate(), stream.rate()]
        .into_iter()
        .find(|rate| rate.numerator() > 0 && rate.denominator() > 0)
        .filter(|rate| (1.0..=480.0).contains(&f64::from(*rate)))
}

/// A stream's language, as its tags say, unless they say it has none
/// (`zxx`) or it's unknown (`und`).
pub fn language(metadata: &DictionaryRef<'_>) -> Option<String> {
    tag(metadata, c"language").filter(|code| !matches!(code.as_str(), "und" | "zxx"))
}

/// The shape of `stream`'s pixels, width over height, where its container
/// gives one.
pub fn container_aspect(stream: &Stream) -> Option<Rational> {
    // SAFETY: the stream is valid while borrowed, and the field a plain pair
    // of numbers.
    shape(unsafe { (*stream.as_ptr()).sample_aspect_ratio })
}

/// The shape of the pixels `parameters`' pictures are coded with, where
/// they give one.
pub fn coded_aspect(parameters: &codec::Parameters) -> Option<Rational> {
    // SAFETY: as above, for the parameters.
    shape(unsafe { (*parameters.as_ptr()).sample_aspect_ratio })
}

/// `ratio`, where it's a shape: above nothing either way.
fn shape(ratio: ffi::AVRational) -> Option<Rational> {
    (ratio.num > 0 && ratio.den > 0).then(|| ratio.into())
}

#[cfg(test)]
#[path = "tests/stream.rs"]
mod tests;
