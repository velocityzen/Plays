use std::ffi::CStr;
use std::ptr;

use ffmpeg_next::codec::subtitle::{Ass, Text};
use ffmpeg_next::{Rational, codec, decoder, ffi};

use super::{ass_line, plain_text, subtitle_decoder};

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

/// Codec parameters of SubRip text.
fn subrip() -> codec::Parameters {
    let mut parameters = codec::Parameters::new();
    // SAFETY: the parameters are ours; these are FFmpeg's numbers.
    unsafe {
        let raw = parameters.as_mut_ptr();
        (*raw).codec_type = ffi::AVMediaType::AVMEDIA_TYPE_SUBTITLE;
        (*raw).codec_id = ffi::AVCodecID::AV_CODEC_ID_SUBRIP;
    }
    parameters
}

/// The encoding `decoder` converts its text from, and the time base it
/// takes packets in.
fn converting(decoder: &decoder::Subtitle) -> (Option<String>, Rational) {
    // SAFETY: the decoder is open while borrowed; its encoding is null or a
    // terminated string it keeps.
    unsafe {
        let raw = decoder.as_ptr();
        let encoding = (*raw).sub_charenc;
        (
            (!encoding.is_null()).then(|| CStr::from_ptr(encoding).to_string_lossy().into_owned()),
            (*raw).pkt_timebase.into(),
        )
    }
}

#[test]
fn a_subtitle_decoder_converts_from_its_encoding_where_ffmpeg_can() {
    let time_base = Rational::new(1, 1000);
    let open = |encoding| converting(&subtitle_decoder(&subrip(), time_base, encoding).unwrap());
    assert_eq!(
        open(Some("WINDOWS-1252")),
        (Some("WINDOWS-1252".to_owned()), time_base)
    );
    assert_eq!(
        open(Some("no such encoding")),
        (None, time_base),
        "as it is"
    );
    assert_eq!(open(None), (None, time_base));
    let none = codec::Parameters::new();
    assert!(subtitle_decoder(&none, time_base, None).is_err());
}
