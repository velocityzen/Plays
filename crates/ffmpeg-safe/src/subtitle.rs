//! A subtitle's text. ffmpeg-next's `Ass::get` and `Text::get` take a
//! rect's bytes for UTF-8 without looking, and read a rect without text
//! from a null pointer.

use std::borrow::Cow;
use std::ffi::{CStr, c_char};

use ffmpeg_next::codec::subtitle::{Ass, Text};

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

#[cfg(test)]
#[path = "tests/subtitle.rs"]
mod tests;
