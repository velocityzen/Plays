use std::path::{Path, PathBuf};

use ffmpeg_next::codec::Id;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::{Dictionary, Rational, ffi, media};

use super::{Described, coded_aspect, container_aspect, frame_rate, language};
use crate::input_file;

/// A fixture of the workspace's.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Subtitles are text where FFmpeg's table says so: TTML, SAMI, MicroDVD
/// and broadcast captions too, never pictures.
#[test]
fn takes_subtitles_for_text_as_ffmpeg_does() {
    for id in [
        Id::SUBRIP,
        Id::ASS,
        Id::MOV_TEXT,
        Id::WEBVTT,
        Id::TTML,
        Id::SAMI,
        Id::MICRODVD,
        Id::EIA_608,
    ] {
        assert!(Described::of(id).text, "{id:?}");
    }
    for id in [
        Id::HDMV_PGS_SUBTITLE,
        Id::DVD_SUBTITLE,
        Id::DVB_SUBTITLE,
        Id::XSUB,
        Id::H264,
    ] {
        assert!(!Described::of(id).text, "{id:?}");
    }
}

#[test]
fn describes_a_codec_by_its_id_or_its_name() {
    let h264 = Described::of(Id::H264);
    assert_eq!(
        h264.long_name.as_deref(),
        Some("H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10")
    );
    assert_eq!(Described::named("h264"), h264);
    assert!(Described::named("subrip").text);
    assert!(!Described::named("hdmv_pgs_subtitle").text);
    assert_eq!(Described::named("no such codec"), Described::default());
    assert_eq!(Described::named("sub\0rip"), Described::default());
    assert_eq!(Described::of(Id::None), Described::default());
}

/// The fixture's video, its stream's frame rates and pixels' shape set as
/// given.
fn video(set: impl FnOnce(*mut ffi::AVStream)) -> (Input, usize) {
    let mut input = input_file(&fixture("clip.mp4")).unwrap();
    let index = input.streams().best(media::Type::Video).unwrap().index();
    let mut stream = input.stream_mut(index).unwrap();
    // SAFETY: the stream is ours and open.
    set(unsafe { stream.as_mut_ptr() });
    (input, index)
}

#[test]
fn a_frame_rate_is_its_average_else_its_guess_if_plausible() {
    let rate = |average: (i32, i32), guessed: (i32, i32)| {
        let ratio = |(num, den)| ffi::AVRational { num, den };
        let (input, index) = video(|stream| {
            // SAFETY: plain numbers of an open stream.
            unsafe {
                (*stream).avg_frame_rate = ratio(average);
                (*stream).r_frame_rate = ratio(guessed);
            }
        });
        frame_rate(&input.stream(index).unwrap())
    };
    assert_eq!(rate((25, 1), (50, 1)), Some(Rational::new(25, 1)));
    assert_eq!(rate((0, 0), (30, 1)), Some(Rational::new(30, 1)));
    assert_eq!(rate((-25, 1), (30, 1)), Some(Rational::new(30, 1)));
    assert_eq!(rate((25, 0), (30, 1)), Some(Rational::new(30, 1)));
    assert_eq!(rate((480, 1), (30, 1)), Some(Rational::new(480, 1)));
    assert_eq!(rate((1, 1), (30, 1)), Some(Rational::new(1, 1)));
    assert_eq!(
        rate((0, 0), (30_000, 1001)),
        Some(Rational::new(30_000, 1001))
    );
    assert_eq!(rate((0, 1), (24, 0)), None);
    // Not plausible, whatever the guess says.
    assert_eq!(rate((481, 1), (25, 1)), None);
    assert_eq!(rate((0, 0), (90_000, 1)), None, "too fast");
    assert_eq!(rate((1, 2), (30, 1)), None, "too slow, and not guessed");
    assert_eq!(rate((0, 0), (0, 0)), None);
}

#[test]
fn a_language_is_none_where_tagged_none_or_unknown() {
    let language_of = |code: Option<&str>| {
        let mut tags = Dictionary::new();
        if let Some(code) = code {
            tags.set("language", code);
        }
        language(&tags)
    };
    assert_eq!(language_of(Some("eng")).as_deref(), Some("eng"));
    assert_eq!(language_of(Some(" fre ")).as_deref(), Some("fre"));
    assert_eq!(language_of(Some("und")), None);
    assert_eq!(language_of(Some("zxx")), None);
    assert_eq!(language_of(None), None);
}

#[test]
fn a_streams_pixels_are_shaped_as_its_container_or_its_pictures_say() {
    let shapes = |container: (i32, i32), coded: (i32, i32)| {
        let (input, index) = video(|stream| {
            // SAFETY: plain numbers of an open stream and its parameters.
            unsafe {
                (*stream).sample_aspect_ratio = ffi::AVRational {
                    num: container.0,
                    den: container.1,
                };
                (*(*stream).codecpar).sample_aspect_ratio = ffi::AVRational {
                    num: coded.0,
                    den: coded.1,
                };
            }
        });
        let stream = input.stream(index).unwrap();
        (
            container_aspect(&stream),
            coded_aspect(&stream.parameters()),
        )
    };
    assert_eq!(
        shapes((4, 3), (8, 9)),
        (Some(Rational::new(4, 3)), Some(Rational::new(8, 9)))
    );
    assert_eq!(shapes((0, 1), (1, 0)), (None, None));
    assert_eq!(shapes((-4, 3), (4, -3)), (None, None));
}
