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
        base_hdr: None,
        ipt: false,
        dolby_vision: None,
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

/// `codec` in 10 bits, its HDR `hdr` and, without Dolby Vision, `base_hdr`;
/// in IPT, `ipt`.
fn hdr_media(codec: VideoCodec, hdr: Hdr, base_hdr: Option<Hdr>, ipt: bool) -> MediaInfo {
    let mut media = with_pictures(codec);
    if let StreamKind::Video(video) = &mut media.streams[0].kind {
        video.bit_depth = 10;
        video.hdr = Some(hdr);
        video.base_hdr = base_hdr;
        video.ipt = ipt;
    }
    media
}

/// HDR stays HDR in a re-encoding only as the device shows it, 10-bit:
/// Dolby Vision as its base layer, HDR10 or HLG, as its reshaping is lost,
/// or as the HDR10 its IPT is reshaped into. Else the plan keeps none, and
/// the picture is fitted to SDR.
#[test]
fn keeps_hdr_in_a_re_encoding_only_as_the_device_shows_it() {
    let media = |hdr, base_hdr| hdr_media(VideoCodec::Av1, hdr, base_hdr, false);
    let device = |hdr: &[Hdr]| Profile {
        video: vec![VideoCodec::H264, VideoCodec::Hevc],
        max_bit_depth: 10,
        hdr: hdr.to_vec(),
        ..Profile::default()
    };
    let kept = |media: &MediaInfo, profile: Profile| {
        let request = CastRequest::new(Source::File("film.mkv".into()), profile);
        match plan(media, &request).unwrap().video {
            Some(VideoPlan::Encode { ten_bit, hdr, .. }) => (ten_bit, hdr),
            other => panic!("{other:?}"),
        }
    };

    let hdr10 = media(Hdr::Hdr10, Some(Hdr::Hdr10));
    assert_eq!(
        kept(&hdr10, device(&[Hdr::Hdr10])),
        (true, Some(Hdr::Hdr10))
    );
    assert_eq!(kept(&hdr10, device(&[Hdr::Hlg])), (true, None));
    assert_eq!(kept(&hdr10, device(&[])), (true, None));
    assert_eq!(kept(&hdr10, Profile::default()), (false, None));

    let profile_8_1 = media(Hdr::DolbyVision, Some(Hdr::Hdr10));
    let every = [Hdr::Hdr10, Hdr::Hlg, Hdr::DolbyVision];
    assert_eq!(kept(&profile_8_1, device(&every)), (true, Some(Hdr::Hdr10)));
    assert_eq!(
        kept(&profile_8_1, device(&[Hdr::Hdr10])),
        (true, Some(Hdr::Hdr10))
    );
    let profile_8_4 = media(Hdr::DolbyVision, Some(Hdr::Hlg));
    assert_eq!(kept(&profile_8_4, device(&[Hdr::Hdr10])), (true, None));
    assert_eq!(kept(&profile_8_4, device(&every)), (true, Some(Hdr::Hlg)));
    let profile_8_2 = media(Hdr::DolbyVision, None);
    assert_eq!(kept(&profile_8_2, device(&every)), (true, None));
    let profile_10_0 = hdr_media(VideoCodec::Av1, Hdr::DolbyVision, None, true);
    assert_eq!(
        kept(&profile_10_0, device(&every)),
        (true, Some(Hdr::Hdr10))
    );
    assert_eq!(
        kept(&profile_10_0, device(&[Hdr::Hdr10])),
        (true, Some(Hdr::Hdr10))
    );
    assert_eq!(kept(&profile_10_0, device(&[Hdr::Hlg])), (true, None));
    assert_eq!(kept(&profile_10_0, Profile::default()), (false, None));
}

/// Repackaged, Dolby Vision loses its signalling, and a device shows its
/// base layer: copied where that's HDR10 the device shows, IPT re-encoded,
/// as without its RPU it's green and purple. A file the device plays as it
/// is keeps its Dolby Vision, for one that shows it. The HDR shown, which
/// the master playlist names, is the base layer's or the encoding's.
#[test]
fn repackages_dolby_vision_as_its_base_layer() {
    let apple_tv = |hdr: &[Hdr]| Profile {
        files: Vec::new(),
        hls: Some(Segments::Fmp4),
        video: vec![VideoCodec::H264, VideoCodec::Hevc],
        max_bit_depth: 10,
        hdr: hdr.to_vec(),
        ..Profile::default()
    };
    let planned = |media: &MediaInfo, profile: Profile| {
        let request = CastRequest::new(Source::File("film.mkv".into()), profile);
        plan(media, &request).unwrap()
    };
    let every = [Hdr::Hdr10, Hdr::Hlg, Hdr::DolbyVision];
    let profile_8_1 = hdr_media(VideoCodec::Hevc, Hdr::DolbyVision, Some(Hdr::Hdr10), false);
    let profile_20 = hdr_media(VideoCodec::Hevc, Hdr::DolbyVision, None, true);

    for hdr in [&every[..], &[Hdr::Hdr10]] {
        let copied = planned(&profile_8_1, apple_tv(hdr));
        assert!(
            matches!(copied.video, Some(VideoPlan::Copy { .. })),
            "{copied:?}"
        );
        assert_eq!(copied.shown_hdr(&profile_8_1), Some(Hdr::Hdr10));
    }
    let profile_8_4 = hdr_media(VideoCodec::Hevc, Hdr::DolbyVision, Some(Hdr::Hlg), false);
    let copied = planned(&profile_8_4, apple_tv(&every));
    assert!(matches!(copied.video, Some(VideoPlan::Copy { .. })));
    assert_eq!(copied.shown_hdr(&profile_8_4), Some(Hdr::Hlg));
    let sdr_copy = planned(&profile_8_1, apple_tv(&[]));
    assert!(matches!(
        sdr_copy.video,
        Some(VideoPlan::Encode { hdr: None, .. })
    ));
    assert_eq!(sdr_copy.shown_hdr(&profile_8_1), None);
    let reshaped = planned(&profile_20, apple_tv(&every));
    assert!(
        matches!(
            reshaped.video,
            Some(VideoPlan::Encode {
                to: VideoCodec::Hevc,
                ten_bit: true,
                hdr: Some(Hdr::Hdr10),
                ..
            })
        ),
        "{reshaped:?}"
    );
    assert_eq!(reshaped.shown_hdr(&profile_20), Some(Hdr::Hdr10));
    let fitted = planned(&profile_20, apple_tv(&[]));
    assert!(matches!(
        fitted.video,
        Some(VideoPlan::Encode { hdr: None, .. })
    ));
    assert_eq!(fitted.shown_hdr(&profile_20), None);

    let dolby_vision_tv = Profile {
        files: vec![Container::Matroska],
        video: vec![VideoCodec::Hevc],
        max_bit_depth: 10,
        hdr: every.to_vec(),
        ..Profile::default()
    };
    let of_profile = |media: &MediaInfo, profile: Option<u8>| {
        let mut media = media.clone();
        if let StreamKind::Video(video) = &mut media.streams[0].kind {
            video.dolby_vision = profile;
        }
        media
    };
    assert_eq!(
        planned(&of_profile(&profile_20, Some(20)), dolby_vision_tv.clone()).delivery,
        Delivery::File
    );

    // One that lists the profiles it shows gets a file as it is only of
    // those, the others as one without Dolby Vision does.
    let listing = Profile {
        dolby_vision: Some(vec![5, 8]),
        ..dolby_vision_tv
    };
    for (media, profile) in [(&profile_20, 5), (&profile_8_1, 8)] {
        let shown = planned(&of_profile(media, Some(profile)), listing.clone());
        assert_eq!(shown.delivery, Delivery::File, "{profile}");
    }
    for profile in [Some(20), None] {
        let reshaped = planned(&of_profile(&profile_20, profile), listing.clone());
        assert_ne!(reshaped.delivery, Delivery::File, "{profile:?}");
        assert!(
            matches!(
                reshaped.video,
                Some(VideoPlan::Encode {
                    hdr: Some(Hdr::Hdr10),
                    ..
                })
            ),
            "{reshaped:?}"
        );
    }
    let hdr10 = hdr_media(VideoCodec::Hevc, Hdr::Hdr10, Some(Hdr::Hdr10), false);
    assert_eq!(planned(&hdr10, listing).delivery, Delivery::File);
}
