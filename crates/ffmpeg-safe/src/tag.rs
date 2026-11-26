//! A tag's value, as text. ffmpeg-next's `DictionaryRef` takes every value
//! for UTF-8 without looking, and demuxers copy what files hold: a WAV's
//! INFO, a Matroska name, in whatever encoding it was written in.

use std::borrow::Cow;
use std::ffi::{CStr, c_int};
use std::ptr;

use ffmpeg_next::{DictionaryRef, ffi};

/// The tag `key` in `metadata`, without the spaces around it, if it has one
/// that isn't blank. One that isn't UTF-8 is read in the encoding it looks
/// most like.
pub fn tag(metadata: &DictionaryRef<'_>, key: &CStr) -> Option<String> {
    // SAFETY: the dictionary is borrowed, so open; FFmpeg takes a null one
    // as empty, and an entry's value is a nul-terminated string it keeps.
    let value = unsafe {
        let entry = ffi::av_dict_get(metadata.as_ptr(), key.as_ptr(), ptr::null(), 0);
        if entry.is_null() {
            return None;
        }
        CStr::from_ptr((*entry).value).to_bytes()
    };
    let text = text(value);
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Every tag in `metadata`, in its order, each read as `tag` reads it,
/// its name too, and those that are blank left out.
pub fn tags(metadata: &DictionaryRef<'_>) -> Vec<(String, String)> {
    let mut tags = Vec::new();
    let mut entry = ptr::null();
    loop {
        // SAFETY: as for `tag`; an empty key with IGNORE_SUFFIX matches
        // every entry, each after the one before it.
        let (key, value) = unsafe {
            entry = ffi::av_dict_get(
                metadata.as_ptr(),
                c"".as_ptr(),
                entry,
                ffi::AV_DICT_IGNORE_SUFFIX as c_int,
            );
            if entry.is_null() {
                break;
            }
            (
                CStr::from_ptr((*entry).key).to_bytes(),
                CStr::from_ptr((*entry).value).to_bytes(),
            )
        };
        let value = text(value);
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            tags.push((text(key).into_owned(), trimmed.to_owned()));
        }
    }
    tags
}

/// `bytes` as UTF-8 if they are, or else decoded as what they look like.
fn text(bytes: &[u8]) -> Cow<'_, str> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(text);
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    detector
        .guess(None, false)
        .decode_without_bom_handling(bytes)
        .0
}

#[cfg(test)]
#[path = "tests/tag.rs"]
mod tests;
