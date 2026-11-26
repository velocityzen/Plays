use super::*;

#[test]
fn fits_the_picture_in_even_numbers() {
    assert_eq!(fit_size(1920, 1080, None), (1920, 1080));
    assert_eq!(fit_size(3840, 2160, Some((1920, 1080))), (1920, 1080));
    assert_eq!(fit_size(1920, 800, Some((1280, 720))), (1280, 534));
    assert_eq!(fit_size(161, 121, None), (162, 122));
}

/// A Matroska file of 720p `codec`, AAC stereo and picture subtitles.
fn with_pictures(codec: VideoCodec) -> MediaInfo {
    let stream = |index, kind| crate::media::StreamInfo {
        index,
        kind,
        language: None,
        title: None,
        default: false,
    };
    let video = VideoInfo {
        codec,
        width: 1280,
        height: 720,
        bit_depth: 8,
        frame_rate: Some(24.0),
        hdr: None,
        picture: false,
    };
    let audio = crate::media::AudioInfo {
        codec: AudioCodec::Aac,
        channels: 2,
        sample_rate: 48_000,
    };
    MediaInfo {
        container: Container::Matroska,
        duration: Some(10.0),
        start: 0.0,
        bit_rate: None,
        streams: vec![
            stream(0, StreamKind::Video(video)),
            stream(1, StreamKind::Audio(audio)),
            stream(2, StreamKind::Subtitle { text: false }),
        ],
    }
}

#[test]
fn re_encodes_video_for_picture_subtitles_where_it_can() {
    let source = Source::File("film.mkv".into());
    let pictures = SubtitleSource::Track(2);
    let planned = |media: &MediaInfo, profile: &Profile, subtitles| {
        plan(media, &source, Pick::Best, Pick::Best, subtitles, profile).unwrap()
    };

    let h264 = with_pictures(VideoCodec::H264);
    let drawn = planned(&h264, &Profile::default(), Some(&pictures));
    assert!(matches!(
        drawn.video,
        Some(VideoPlan::Encode {
            to: VideoCodec::H264,
            ..
        })
    ));
    assert_eq!(drawn.subtitles, Some(2));
    let plain = planned(&h264, &Profile::default(), None);
    assert!(matches!(plain.video, Some(VideoPlan::Copy { .. })));
    assert_eq!(plain.subtitles, None);
    // One that plays the file itself gets it re-encoded for them too.
    let mkv = Profile {
        files: vec![Container::Matroska],
        ..Profile::default()
    };
    assert_eq!(planned(&h264, &mkv, None).delivery, Delivery::File);
    let drawn = planned(&h264, &mkv, Some(&pictures));
    assert_eq!(drawn.delivery, Delivery::Hls(Segments::Ts));
    assert_eq!(drawn.subtitles, Some(2));

    // A device that decodes nothing this computer encodes gets it copied.
    let vp9 = Profile {
        hls: Some(Segments::Fmp4),
        video: vec![VideoCodec::Vp9],
        ..Profile::default()
    };
    let copied = planned(&with_pictures(VideoCodec::Vp9), &vp9, Some(&pictures));
    assert!(matches!(
        copied.video,
        Some(VideoPlan::Copy {
            codec: VideoCodec::Vp9,
            ..
        })
    ));
    assert_eq!(copied.subtitles, None);
    assert_eq!(copied.delivery, Delivery::Hls(Segments::Fmp4));

    // Sound alone has nothing to draw them in, and goes as it would.
    let mut sound = with_pictures(VideoCodec::H264);
    sound.container = Container::Mp4;
    sound.streams.remove(0);
    let alone = planned(&sound, &Profile::default(), Some(&pictures));
    assert_eq!(alone.delivery, Delivery::File);
    assert_eq!(alone.subtitles, None);
}
