use ffmpeg_next::{codec, media};

use super::*;
use crate::ui::testing::fixture;

/// What the console is told of the video in `name`, in `tests/fixtures`.
fn of(name: &str) -> VideoDetails {
    ffmpeg::init().unwrap();
    let input = ffmpeg::format::input(&fixture(name)).unwrap();
    let stream = input.streams().best(media::Type::Video).unwrap();
    let context = codec::Context::from_parameters(stream.parameters()).unwrap();
    VideoDetails::of(&context.decoder().video().unwrap())
}

#[test]
fn a_video_says_if_its_dolby_vision() {
    let profile_20 = of("clip-dv-ipt.mp4");
    assert!(profile_20.dolby_vision);
    assert_eq!(profile_20.transfer, Some("smpte2084"));
    assert!(!of("clip-hdr10.mkv").dolby_vision);
    assert!(!of("clip.mp4").dolby_vision);
}

#[test]
fn hdr_is_pq_hlg_or_dolby_vision_whatever_its_transfer() {
    let with = |transfer, dolby_vision| VideoDetails {
        transfer,
        dolby_vision,
        ..VideoDetails::default()
    };
    assert!(with(Some("smpte2084"), false).hdr());
    assert!(with(Some("arib-std-b67"), false).hdr());
    assert!(with(None, true).hdr(), "profile 5's says nothing");
    assert!(with(Some("bt709"), true).hdr(), "profile 8.2's SDR base");
    assert!(!with(Some("bt709"), false).hdr());
    assert!(!with(None, false).hdr());
}
