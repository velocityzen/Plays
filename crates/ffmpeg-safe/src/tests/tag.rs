use std::ffi::CStr;
use std::ptr;

use ffmpeg_next::{DictionaryRef, ffi};

use super::tag;

/// A dictionary holding `entries` as they are, as a demuxer copies them.
struct Raw(*mut ffi::AVDictionary);

impl Raw {
    fn of(entries: &[(&CStr, &CStr)]) -> Self {
        let mut dictionary = ptr::null_mut();
        for (key, value) in entries {
            // SAFETY: FFmpeg copies the key and the value.
            let set =
                unsafe { ffi::av_dict_set(&raw mut dictionary, key.as_ptr(), value.as_ptr(), 0) };
            assert!(set >= 0);
        }
        Self(dictionary)
    }

    fn tag(&self, key: &CStr) -> Option<String> {
        // SAFETY: the dictionary lives as long as this borrow.
        let metadata = unsafe { DictionaryRef::wrap(self.0) };
        tag(&metadata, key)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        // SAFETY: made by av_dict_set, and freed only here.
        unsafe { ffi::av_dict_free(&raw mut self.0) };
    }
}

#[test]
fn reads_a_tag_in_utf8_as_it_is() {
    let tags = Raw::of(&[(c"title", c"  Caf\xc3\xa9 "), (c"artist", c"   ")]);
    assert_eq!(tags.tag(c"title").as_deref(), Some("Café"));
    assert_eq!(tags.tag(c"TITLE").as_deref(), Some("Café"));
    assert_eq!(tags.tag(c"artist"), None);
    assert_eq!(tags.tag(c"album"), None);
}

#[test]
fn reads_a_tag_in_another_encoding_as_what_it_looks_like() {
    let tags = Raw::of(&[
        (c"title", c"Caf\xe9"),
        (c"artist", c"M\xfcller"),
        (c"album", c"\xcf\xf0\xe8\xe2\xe5\xf2"),
        (c"comment", c"\xe9"),
    ]);
    assert_eq!(tags.tag(c"title").as_deref(), Some("Café"));
    assert_eq!(tags.tag(c"artist").as_deref(), Some("Müller"));
    assert_eq!(tags.tag(c"album").as_deref(), Some("Привет"));
    assert_eq!(tags.tag(c"comment").as_deref(), Some("é"));
}

#[test]
fn reads_no_tags_from_no_dictionary() {
    assert_eq!(Raw(ptr::null_mut()).tag(c"title"), None);
}
