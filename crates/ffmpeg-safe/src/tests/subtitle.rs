use std::ffi::CStr;
use std::ptr;

use ffmpeg_next::codec::subtitle::{Ass, Text};
use ffmpeg_next::ffi;

use super::{ass_line, plain_text};

/// What `ass_line` and `plain_text` read of a rect holding `ass` as its
/// ASS line and `text` as its plain text, as a decoder leaves it; null
/// where none.
fn read(ass: Option<&CStr>, text: Option<&CStr>) -> (String, String) {
    let pointer =
        |text: Option<&CStr>| text.map_or(ptr::null_mut(), |text| text.as_ptr().cast_mut());
    // SAFETY: a rect is numbers and pointers, all of them zero or null as
    // FFmpeg allocates it.
    let mut rect: ffi::AVSubtitleRect = unsafe { std::mem::zeroed() };
    rect.ass = pointer(ass);
    rect.text = pointer(text);
    // SAFETY: the rect, and the text it points to, live till both are read.
    let (ass, plain) = unsafe { (Ass::wrap(&rect), Text::wrap(&rect)) };
    (ass_line(&ass).into_owned(), plain_text(&plain).into_owned())
}

#[test]
fn reads_a_rects_text() {
    assert_eq!(
        read(Some(c"0,0,Default,,0,0,0,,Grüße"), Some(c"Hello, world")),
        ("0,0,Default,,0,0,0,,Grüße".into(), "Hello, world".into())
    );
}

#[test]
fn replaces_what_isnt_utf8() {
    // Latin-1, and a sequence cut short.
    assert_eq!(
        read(Some(c"Gr\xfc\xdfe"), Some(c"\xe2\x82 world")),
        ("Gr\u{fffd}\u{fffd}e".into(), "\u{fffd} world".into())
    );
}

#[test]
fn a_rect_without_text_reads_as_empty() {
    assert_eq!(read(None, None), (String::new(), String::new()));
}
