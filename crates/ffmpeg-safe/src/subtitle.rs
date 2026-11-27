//! A subtitle's text, and a decoder for it. ffmpeg-next's `Ass::get` and
//! `Text::get` take a rect's bytes for UTF-8 without looking, and read a
//! rect without text from a null pointer.

use std::borrow::Cow;
use std::ffi::{CStr, c_char};

use ffmpeg_next::codec::subtitle::{Ass, Text};
use ffmpeg_next::{Dictionary, Error, Rational, codec, decoder};

/// An ASS rect's event line. Bytes that aren't UTF-8 are replaced.
pub fn ass_line<'a>(rect: &Ass<'a>) -> Cow<'a, str> {
    // SAFETY: the rect is the subtitle's, which outlives `'a`.
    unsafe { read((*rect.as_ptr()).ass) }
}

/// A plain text rect's text. Bytes that aren't UTF-8 are replaced.
pub fn plain_text<'a>(rect: &Text<'a>) -> Cow<'a, str> {
    // SAFETY: as in `ass_line`.
    unsafe { read((*rect.as_ptr()).text) }
}

/// The text at `text`, none if it's null.
///
/// # Safety
///
/// `text` is null, or a nul-terminated string that outlives `'a`.
unsafe fn read<'a>(text: *const c_char) -> Cow<'a, str> {
    if text.is_null() {
        return Cow::Borrowed("");
    }
    // SAFETY: the caller's.
    String::from_utf8_lossy(unsafe { CStr::from_ptr(text) }.to_bytes())
}

/// A decoder for a subtitle stream, converting its text from `encoding`
/// (an iconv name) where FFmpeg can, and as it is where it can't.
pub fn subtitle_decoder(
    parameters: &codec::Parameters,
    time_base: Rational,
    encoding: Option<&str>,
) -> Result<decoder::Subtitle, Error> {
    let open = |encoding: Option<&str>| {
        let context = codec::context::Context::from_parameters(parameters.clone())?;
        let mut decoder = context.decoder();
        // Lets FFmpeg take a cue's end from its packet's duration.
        decoder.set_packet_time_base(time_base);
        let codec = decoder::find(parameters.id()).ok_or(Error::DecoderNotFound)?;
        let mut options = Dictionary::new();
        if let Some(encoding) = encoding {
            options.set("sub_charenc", encoding);
        }
        decoder.open_as_with(codec, options)?.subtitle()
    };
    match encoding {
        Some(encoding) => open(Some(encoding)).or_else(|err| {
            log::warn!("can't convert subtitles from {encoding}: {err}");
            open(None)
        }),
        None => open(None),
    }
}

#[cfg(test)]
#[path = "tests/subtitle.rs"]
mod tests;
