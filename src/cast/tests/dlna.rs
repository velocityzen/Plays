use super::*;

#[test]
fn media_starts_at_its_url_from_where_it_is_to_start() {
    let media = Media {
        url: "http://plays/1/sound.aac".into(),
        kind: Kind::Audio,
        mime: "audio/aac".into(),
        features: None,
        title: None,
        duration: Some(10.0),
        size: None,
        subtitles: None,
    };
    let from = starting_at(&media, 2.5);
    assert_eq!(from.url, "http://plays/1/sound.aac?start=2.500");
    assert_eq!(from.duration, Some(7.5), "what's left of it");
    assert_eq!(starting_at(&media, 0.0), media);
    let queried = Media {
        url: "http://plays/1/sound.aac?token=1".into(),
        ..media
    };
    assert_eq!(
        starting_at(&queried, 1.0).url,
        "http://plays/1/sound.aac?token=1&start=1.000"
    );
}

fn listed(name: &str) -> Option<Vec<Capability>> {
    let list = match name {
        "samsung" => include_str!("../../../crates/dlna/fixtures/samsung-protocol-info.txt"),
        "lg" => include_str!("../../../crates/dlna/fixtures/lg-protocol-info.txt"),
        "sonos" => include_str!("../../../crates/dlna/fixtures/sonos-protocol-info.txt"),
        "devialet" => include_str!("../../../crates/dlna/fixtures/devialet-protocol-info.txt"),
        _ => unreachable!(),
    };
    Some(Capability::parse_list(list))
}

#[test]
fn a_tv_shows_video_and_a_speaker_sound() {
    assert_eq!(shows(listed("samsung").as_deref()), Some(Shows::Video));
    assert_eq!(shows(listed("lg").as_deref()), Some(Shows::Video));
    assert_eq!(shows(listed("sonos").as_deref()), Some(Shows::Sound));
    assert_eq!(shows(listed("devialet").as_deref()), Some(Shows::Sound));
    assert_eq!(shows(None), Some(Shows::Video), "one that doesn't say");
    let frame = Capability::parse_list("http-get:*:image/jpeg:*,http-get:*:image/png:*");
    assert_eq!(shows(Some(&frame)), None, "a picture frame");
    let elsewhere = Capability::parse_list("rtsp-rtp-udp:*:video/H264:*");
    assert_eq!(shows(Some(&elsewhere)), None, "nothing over HTTP");
}

#[test]
fn a_tv_gets_the_files_it_lists_and_else_a_stream() {
    let samsung = profile(listed("samsung").as_deref());
    assert_eq!(
        samsung.files,
        [
            Container::Mp4,
            Container::MpegTs,
            Container::Matroska,
            Container::Avi,
            Container::Mp3,
            Container::Flac,
        ]
    );
    assert!(samsung.stream);
    assert!(
        samsung.sound_stream.is_empty(),
        "a TV gets the MPEG-TS stream"
    );
    assert_eq!(samsung.hls, None, "MPEG-TS rather than HLS");
    assert!(!samsung.fetches_urls);
    assert_eq!(samsung.video, [VideoCodec::H264, VideoCodec::Mpeg2]);
    assert_eq!(
        samsung.audio,
        [
            AudioCodec::Aac,
            AudioCodec::Mp3,
            AudioCodec::Flac,
            AudioCodec::Pcm
        ]
    );
    assert_eq!(samsung.max_channels, 6, "AAC_MULT5");
    assert_eq!(samsung.max_size, Some((1920, 1080)));

    let lg = profile(listed("lg").as_deref());
    assert_eq!(
        lg.files,
        [
            Container::Mp4,
            Container::MpegTs,
            Container::Matroska,
            Container::WebM,
            Container::Mp3,
            Container::Aac,
            Container::Flac,
        ]
    );
    assert_eq!(
        lg.video,
        [
            VideoCodec::H264,
            VideoCodec::Mpeg2,
            VideoCodec::Vp9,
            VideoCodec::Vp8
        ]
    );
    assert_eq!(
        lg.audio,
        [
            AudioCodec::Aac,
            AudioCodec::Mp3,
            AudioCodec::Ac3,
            AudioCodec::Flac,
            AudioCodec::Vorbis,
            AudioCodec::Opus
        ]
    );
    assert_eq!(lg.max_channels, 6, "AC3");
}

#[test]
fn a_speaker_gets_files_and_a_stream_of_its_sound() {
    let sonos = profile(listed("sonos").as_deref());
    assert_eq!(
        sonos.files,
        [
            Container::Mp4,
            Container::Mp3,
            Container::Flac,
            Container::Wav
        ]
    );
    assert!(!sonos.stream, "no stream of MPEG-TS for a speaker");
    assert_eq!(sonos.sound_stream, [Container::Flac], "it lists no AAC");
    assert_eq!(sonos.hls, None);
    assert_eq!(
        sonos.audio,
        [
            AudioCodec::Aac,
            AudioCodec::Mp3,
            AudioCodec::Flac,
            AudioCodec::Pcm
        ]
    );

    let devialet = profile(listed("devialet").as_deref());
    assert!(!devialet.stream);
    assert_eq!(devialet.sound_stream, [Container::Aac, Container::Flac]);
    assert_eq!(
        devialet.files,
        [
            Container::Mp4,
            Container::Mp3,
            Container::Aac,
            Container::Flac,
            Container::Ogg,
            Container::Wav,
        ]
    );
    assert!(devialet.audio.contains(&AudioCodec::Opus));
    assert!(devialet.audio.contains(&AudioCodec::Ac3), "audio/x-ac3");
}

#[test]
fn one_that_doesnt_say_gets_h264_and_aac_in_mpeg_ts() {
    let anything = Capability::parse_list("http-get:*:*:*");
    for profile in [profile(None), profile(Some(&anything))] {
        assert_eq!(profile.files, [Container::MpegTs]);
        assert!(profile.stream);
        assert_eq!(profile.hls, None);
        assert_eq!(profile.video, [VideoCodec::H264]);
        assert_eq!(profile.audio, [AudioCodec::Aac]);
        assert_eq!(profile.max_size, Some((1920, 1080)));
    }
}

#[test]
fn a_4k_tv_gets_4k() {
    let tv = Capability::parse_list(
        "http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD_AAC,\
             http-get:*:video/mp4:DLNA.ORG_PN=HEVC_MP4_MP_UHD_AAC",
    );
    let profile = profile(Some(&tv));
    assert_eq!(profile.max_size, Some((3840, 2160)));
    assert_eq!(profile.video, [VideoCodec::H264, VideoCodec::Hevc]);
}

#[test]
fn a_mime_goes_by_the_name_the_renderer_lists() {
    let samsung = listed("samsung");
    let lg = listed("lg");
    let sonos = listed("sonos");
    let devialet = listed("devialet");
    assert_eq!(named_as("video/mp2t", samsung.as_deref()), "video/mpeg");
    assert_eq!(named_as("video/mp2t", lg.as_deref()), "video/mpeg");
    assert_eq!(named_as("video/mp4", samsung.as_deref()), "video/mp4");
    assert_eq!(named_as("audio/flac", samsung.as_deref()), "audio/x-flac");
    assert_eq!(named_as("audio/mpeg", sonos.as_deref()), "audio/mpeg");
    assert_eq!(named_as("audio/mp4", devialet.as_deref()), "audio/mp4");
    assert_eq!(named_as("audio/aac", lg.as_deref()), "audio/vnd.dlna.adts");
    assert_eq!(
        named_as("video/webm", samsung.as_deref()),
        "video/webm",
        "as it is, where it lists no other name"
    );
    assert_eq!(named_as("video/mp2t", None), "video/mp2t");
}

#[test]
fn the_tiles_volume_scales_the_renderers() {
    let at = |level, muted| Volume { level, muted };
    let mut loudness = Loudness::new(at(0.3, false), 0.5);
    assert!((loudness.full - 0.6).abs() < 1e-6);
    assert!(
        (Loudness::new(at(0.8, false), 0.5).full - 1.0).abs() < 1e-6,
        "never louder than it goes"
    );
    assert!(
        (Loudness::new(at(0.3, false), 0.0).full - 0.3).abs() < 1e-6,
        "a silent tile leaves the renderer where it is"
    );

    loudness.heard(at(0.4, true));
    assert!((loudness.full - 0.8).abs() < 1e-6, "turned on its remote");
    assert!(loudness.muted);
    loudness.heard(at(0.401, false));
    assert!((loudness.full - 0.8).abs() < 1e-6, "as it was set");
}
