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
    let pictures = SubtitleSource::Track(2);
    let planned = |media: &MediaInfo, profile: &Profile, subtitles: Option<&SubtitleSource>| {
        let request = CastRequest {
            subtitles: subtitles.cloned(),
            ..CastRequest::new(Source::File("film.mkv".into()), profile.clone())
        };
        plan(media, &request).unwrap()
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

#[test]
fn re_encodes_video_for_adjustments_at_the_size_it_shows() {
    let h264 = with_pictures(VideoCodec::H264);
    let turned = Adjustments {
        turns: 1,
        ..Adjustments::default()
    };
    let request = |profile: Profile, adjustments| CastRequest {
        adjustments,
        ..CastRequest::new(Source::File("film.mkv".into()), profile)
    };
    let mkv = Profile {
        files: vec![Container::Matroska],
        max_size: None,
        ..Profile::default()
    };
    let plain = plan(&h264, &request(mkv.clone(), None)).unwrap();
    assert_eq!(plain.delivery, Delivery::File);
    assert_eq!(plain.adjustments, None);

    let adjusted = plan(&h264, &request(mkv, Some(turned))).unwrap();
    assert_eq!(adjusted.delivery, Delivery::Hls(Segments::Ts));
    assert!(matches!(
        adjusted.video,
        Some(VideoPlan::Encode {
            width: 720,
            height: 1280,
            ..
        })
    ));
    assert_eq!(adjusted.adjustments, Some(turned));
    // Fitted to the device on its side too.
    let small = Profile {
        max_size: Some((640, 640)),
        ..Profile::default()
    };
    let cropped = Adjustments {
        crop: Some(1.0),
        ..turned
    };
    let fitted = plan(&h264, &request(small, Some(cropped))).unwrap();
    assert!(matches!(
        fitted.video,
        Some(VideoPlan::Encode {
            width: 640,
            height: 640,
            ..
        })
    ));

    // Where it can't be re-encoded, it's copied as it is.
    let vp9 = Profile {
        hls: Some(Segments::Fmp4),
        video: vec![VideoCodec::Vp9],
        ..Profile::default()
    };
    let copied = plan(&with_pictures(VideoCodec::Vp9), &request(vp9, Some(turned))).unwrap();
    assert!(matches!(copied.video, Some(VideoPlan::Copy { .. })));
    assert_eq!(copied.adjustments, None);
}

#[test]
fn re_encoding_video_for_its_keyframes_does_its_adjustments() {
    let h264 = with_pictures(VideoCodec::H264);
    let gamma = Adjustments {
        gamma: 1.5,
        ..Adjustments::default()
    };
    let mut request = CastRequest::new(Source::File("film.mkv".into()), Profile::default());
    let mut plan = plan(&h264, &request).unwrap();
    assert!(matches!(plan.video, Some(VideoPlan::Copy { .. })));
    request.adjustments = Some(gamma);
    re_encode_video(&mut plan, &h264, &request).unwrap();
    assert!(matches!(plan.video, Some(VideoPlan::Encode { .. })));
    assert_eq!(plan.adjustments, Some(gamma));
}

#[test]
fn delays_the_sound_where_the_media_isnt_sent_as_it_is() {
    let mut mp4 = with_pictures(VideoCodec::H264);
    mp4.container = Container::Mp4;
    let request = |profile: Profile, delay| CastRequest {
        audio_delay: delay,
        ..CastRequest::new(Source::File("film.mp4".into()), profile)
    };
    let plain = plan(&mp4, &request(Profile::default(), 0.0)).unwrap();
    assert_eq!(plain.delivery, Delivery::File);
    let delayed = plan(&mp4, &request(Profile::default(), 0.25)).unwrap();
    assert_eq!(delayed.delivery, Delivery::Hls(Segments::Ts));
    assert_eq!(delayed.audio_delay, 0.25);
    assert!(matches!(delayed.video, Some(VideoPlan::Copy { .. })));
    assert!(matches!(delayed.audio, Some(AudioPlan::Copy { .. })));

    // A device that takes only files gets the file, as it is.
    let files = Profile {
        hls: None,
        ..Profile::default()
    };
    let as_is = plan(&mp4, &request(files, -0.5)).unwrap();
    assert_eq!(as_is.delivery, Delivery::File);
    assert_eq!(as_is.audio_delay, 0.0);

    // Without sound, there's none to delay.
    let silent = CastRequest {
        audio: Pick::Off,
        ..request(Profile::default(), 0.25)
    };
    let picture = plan(&mp4, &silent).unwrap();
    assert_eq!(picture.audio_delay, 0.0);
}
