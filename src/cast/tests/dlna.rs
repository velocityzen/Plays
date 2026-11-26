use futures::channel::mpsc::{self, UnboundedSender};

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

/// What `learn` knows, as renderers are found over the channel it's
/// handed.
fn learning() -> (UnboundedSender<Found>, Arc<Mutex<Known>>) {
    let (found, finding) = mpsc::unbounded();
    let known = Arc::new(Mutex::new(Known::default()));
    let learned = Arc::clone(&known);
    thread::spawn(move || futures::executor::block_on(learn(finding, &learned)));
    (found, known)
}

fn learned(known: &Mutex<Known>, id: &str, within: std::time::Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if known.lock().plays.contains_key(id) {
            return true;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[test]
fn a_renderer_slow_to_say_what_it_plays_holds_up_no_other() {
    use dlna::fake::{FakeRenderer, Kind as Renderer};
    let slow = FakeRenderer::start(Renderer::Tv);
    slow.state().answer_after = std::time::Duration::from_secs(5);
    let quick = FakeRenderer::start(Renderer::Speaker);
    let (found, known) = learning();
    found.unbounded_send(Found::Added(slow.device())).unwrap();
    found.unbounded_send(Found::Added(quick.device())).unwrap();
    let within = std::time::Duration::from_secs(3);
    assert!(
        learned(&known, &quick.device().id, within),
        "the quick one waited"
    );
    assert!(!known.lock().plays.contains_key(&slow.device().id));
    assert!(
        learned(&known, &slow.device().id, within),
        "the slow one in time"
    );
}

#[test]
fn a_renderer_gone_while_asked_is_forgotten_once_it_answers() {
    use dlna::fake::{FakeRenderer, Kind as Renderer};
    let tv = FakeRenderer::start(Renderer::Tv);
    tv.state().answer_after = std::time::Duration::from_millis(300);
    let (found, known) = learning();
    found.unbounded_send(Found::Added(tv.device())).unwrap();
    found.unbounded_send(Found::Removed(tv.device())).unwrap();
    assert!(tv.wait_for(|state| !state.received("GetProtocolInfo").is_empty()));
    let answered = std::time::Duration::from_secs(1);
    assert!(!learned(&known, &tv.device().id, answered));
    assert!(known.lock().asking.is_empty());
}

fn listed(name: &str) -> Option<Vec<Capability>> {
    let list = match name {
        "samsung" => include_str!("../../../crates/dlna/fixtures/samsung-protocol-info.txt"),
        "lg" => include_str!("../../../crates/dlna/fixtures/lg-protocol-info.txt"),
        "sonos" => include_str!("../../../crates/dlna/fixtures/sonos-protocol-info.txt"),
        "devialet" => include_str!("../../../crates/dlna/fixtures/devialet-protocol-info.txt"),
        "visionmaster" => {
            include_str!("../../../crates/dlna/fixtures/visionmaster-protocol-info.txt")
        }
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
    let samsung = profile(listed("samsung").as_deref(), None);
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

    let lg = profile(listed("lg").as_deref(), None);
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
    let sonos = profile(listed("sonos").as_deref(), None);
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

    let devialet = profile(listed("devialet").as_deref(), None);
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
    for profile in [profile(None, None), profile(Some(&anything), None)] {
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
    let profile = profile(Some(&tv), None);
    assert_eq!(profile.max_size, Some((3840, 2160)));
    assert_eq!(profile.video, [VideoCodec::H264, VideoCodec::Hevc]);
}

/// What a Google TV projector's screen shows, as AirPlay there says: 4K at
/// most, every HDR, and Dolby Vision's profiles 4, 5, 8 and 9.
fn projector_screen() -> Screen {
    Screen {
        max_size: Some((3840, 2160)),
        hdr: vec![
            ::airplay::Hdr::Hdr10,
            ::airplay::Hdr::DolbyVision,
            ::airplay::Hdr::Hlg,
        ],
        dolby_vision: vec![4, 5, 8, 9],
    }
}

#[test]
fn a_tv_naming_no_codecs_gets_hevc_and_the_hdr_its_screen_shows() {
    let projector = listed("visionmaster");
    let projector = projector.as_deref();
    assert!(names_no_codecs(projector));
    let screen = projector_screen();
    let hdr = profile(projector, Some(&screen));
    assert_eq!(
        hdr.video,
        [
            VideoCodec::H264,
            VideoCodec::Hevc,
            VideoCodec::Vp9,
            VideoCodec::Vp8
        ]
    );
    assert_eq!(hdr.max_bit_depth, 10);
    assert_eq!(hdr.hdr, [Hdr::Hdr10, Hdr::DolbyVision, Hdr::Hlg]);
    assert_eq!(hdr.dolby_vision, Some(vec![4, 5, 8, 9]));
    assert_eq!(hdr.max_size, Some((3840, 2160)));
    assert_eq!(
        hdr.files,
        [
            Container::Mp4,
            Container::MpegTs,
            Container::Matroska,
            Container::WebM,
            Container::Avi,
            Container::Mp3,
            Container::Aac,
            Container::Flac,
            Container::Ogg,
            Container::Wav,
        ]
    );
    assert!(hdr.stream);
    let without_size = Screen {
        max_size: None,
        ..screen.clone()
    };
    let without_size = profile(projector, Some(&without_size));
    assert_eq!(without_size.max_size, Some((1920, 1080)));
    assert_eq!(without_size.hdr, [Hdr::Hdr10, Hdr::DolbyVision, Hdr::Hlg]);
    // Listing no profiles it decodes, Dolby Vision goes as the HDR10 under
    // it, or reshaped into HDR10.
    let no_profiles = Screen {
        dolby_vision: Vec::new(),
        ..screen.clone()
    };
    let no_profiles = profile(projector, Some(&no_profiles));
    assert_eq!(no_profiles.hdr, [Hdr::Hdr10, Hdr::Hlg]);
    assert_eq!(no_profiles.dolby_vision, Some(Vec::new()));
    let hevc = Capability::parse_list("http-get:*:video/mp4:*,http-get:*:video/hevc:*");
    assert_eq!(
        profile(Some(&hevc), Some(&screen)).video,
        [VideoCodec::H264, VideoCodec::Hevc],
        "HEVC once"
    );

    // Without a screen that says, or one showing no HDR it's sent: fitted
    // to SDR.
    let showing = |hdr: Vec<::airplay::Hdr>, dolby_vision: Vec<u8>| Screen {
        hdr,
        dolby_vision,
        ..screen.clone()
    };
    for plain in [
        profile(projector, None),
        profile(projector, Some(&showing(Vec::new(), vec![5]))),
        profile(
            projector,
            Some(&showing(vec![::airplay::Hdr::DolbyVision], Vec::new())),
        ),
    ] {
        assert_eq!(
            plain.video,
            [VideoCodec::H264, VideoCodec::Vp9, VideoCodec::Vp8]
        );
        assert_eq!(plain.max_bit_depth, 8);
        assert!(plain.hdr.is_empty());
        assert_eq!(plain.dolby_vision, None);
        assert_eq!(plain.max_size, Some((1920, 1080)));
    }
    let dolby_vision_alone = profile(
        projector,
        Some(&showing(vec![::airplay::Hdr::DolbyVision], vec![5])),
    );
    assert_eq!(dolby_vision_alone.hdr, [Hdr::DolbyVision]);
}

#[test]
fn a_tv_naming_codecs_or_none_gets_no_hdr_whatever_its_screen_shows() {
    let screen = projector_screen();
    let anything = Capability::parse_list("http-get:*:*:*");
    let hevc = Capability::parse_list(
        "http-get:*:video/mp4:*,http-get:*:video/mp4:DLNA.ORG_PN=HEVC_MP4_MP_UHD_AAC",
    );
    let speaker = Capability::parse_list("http-get:*:audio/mpeg:*,http-get:*:audio/flac:*");
    let lists = [
        listed("samsung"),
        listed("lg"),
        listed("sonos"),
        None,
        Some(anything),
        Some(hevc),
        Some(speaker),
    ];
    for list in &lists {
        assert!(!names_no_codecs(list.as_deref()), "{list:?}");
        assert_eq!(
            profile(list.as_deref(), Some(&screen)),
            profile(list.as_deref(), None),
            "{list:?}"
        );
    }
}

/// What AirPlay on a Google TV projector answers `GET /info` with.
fn projector_info() -> plist::Dictionary {
    use plist::{Dictionary, Value};
    let mode = |name: &str| {
        let mut mode = Dictionary::new();
        mode.insert("HDRMode".into(), name.into());
        mode
    };
    let mut dolby_vision = mode("DoVi");
    let codecs = ["dvhe.05.09", "hev1.08.09"].map(Value::from);
    dolby_vision.insert("codecStrings".into(), Value::Array(codecs.to_vec()));
    let mut display = Dictionary::new();
    display.insert("widthPixelsMax".into(), Value::Integer(3840.into()));
    display.insert("heightPixelsMax".into(), Value::Integer(2160.into()));
    display.insert(
        "HDRSupportedModes".into(),
        Value::Array(
            [mode("SDR"), mode("HDR10"), dolby_vision, mode("HLG")]
                .map(Value::Dictionary)
                .to_vec(),
        ),
    );
    let mut info = Dictionary::new();
    info.insert(
        "displays".into(),
        Value::Array(vec![Value::Dictionary(display)]),
    );
    info
}

#[test]
fn a_tv_naming_no_codecs_is_asked_by_airplay_what_its_screen_shows() {
    use ::airplay::fake::{FakeReceiver, Kind as Receiver};
    use dlna::fake::{FakeRenderer, Kind as Renderer};

    let airplay = FakeReceiver::start(Receiver::Tv { pin: "1234" });
    airplay.state().info = Some(projector_info());
    let asked = || airplay.state().requests("GET", "/info").len();
    let cast_to = |renderer: &FakeRenderer| {
        let mut target = DlnaTarget::start_at(renderer.search_at());
        target.screens_on = Some(airplay.device().addresses[0].port());
        let mut session = target.connect(&found(&target)).expect("a session");
        let profile = session.profile();
        session.stop().unwrap();
        profile
    };

    let projector = FakeRenderer::start(Renderer::Tv);
    projector.state().sink =
        include_str!("../../../crates/dlna/fixtures/visionmaster-protocol-info.txt").into();
    let served = cast_to(&projector);
    assert_eq!(asked(), 1);
    assert_eq!(served.hdr, [Hdr::Hdr10, Hdr::DolbyVision, Hdr::Hlg]);
    assert_eq!(served.dolby_vision, Some(vec![5, 8]));
    assert_eq!(served.max_size, Some((3840, 2160)));
    assert!(served.video.contains(&VideoCodec::Hevc));

    let samsung = FakeRenderer::start(Renderer::Tv);
    let served = cast_to(&samsung);
    assert_eq!(asked(), 1, "one naming codecs isn't asked");
    assert!(served.hdr.is_empty());

    // Nor is anything where tests run, whose AirPlay may answer.
    let target = DlnaTarget::start_at(projector.search_at());
    assert_eq!(target.screens_on, None);
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
fn the_blocks_volume_scales_the_renderers() {
    let at = |level, muted| Volume { level, muted };
    let mut loudness = Loudness::new(at(0.3, false), 0.5);
    assert!((loudness.full - 0.6).abs() < 1e-6);
    assert!(
        (Loudness::new(at(0.8, false), 0.5).full - 1.0).abs() < 1e-6,
        "never louder than it goes"
    );
    assert!(
        (Loudness::new(at(0.3, false), 0.0).full - 0.3).abs() < 1e-6,
        "a silent block leaves the renderer where it is"
    );

    loudness.heard(at(0.4, true));
    assert!((loudness.full - 0.8).abs() < 1e-6, "turned on its remote");
    assert!(loudness.muted);
    loudness.heard(at(0.401, false));
    assert!((loudness.full - 0.8).abs() < 1e-6, "as it was set");
}

/// The first device `target` finds.
fn found(target: &DlnaTarget) -> Device {
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(device) = target.devices().into_iter().next() {
            return device;
        }
        assert!(Instant::now() < deadline, "the renderer to be found");
        thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// A session with `renderer`, as casting to it starts one.
fn session_with(renderer: &dlna::fake::FakeRenderer) -> Box<dyn Session> {
    let target = DlnaTarget::start_at(renderer.search_at());
    target.connect(&found(&target)).expect("a session")
}

#[test]
fn a_renderer_over_ipv6_is_where_it_was_found() {
    let at = (std::net::Ipv6Addr::LOCALHOST, 0).into();
    let renderer = dlna::fake::FakeRenderer::start_at(dlna::fake::Kind::Tv, at);
    let target = DlnaTarget::start_at(renderer.search_at());
    let device = found(&target);
    assert!(
        renderer
            .location()
            .starts_with(&format!("http://{}/", device.address)),
        "{} at {}",
        renderer.location(),
        device.address
    );
}

/// A clip to play from `start`.
fn clip(start: f64, autoplay: bool) -> Load {
    Load {
        url: "http://192.0.2.1:8080/cast/clip.mp4".into(),
        mime: "video/mp4".into(),
        protocol_info: "http-get:*:video/mp4:*".into(),
        title: "Clip".into(),
        duration: Some(60.0),
        live: false,
        segments: None,
        start,
        autoplay,
        volume: 1.0,
        muted: false,
        subtitles: None,
        picture_here: false,
        starts_at_url: false,
    }
}

/// The first status `wanted` picks, asked for as often as can be.
fn status_until(session: &mut dyn Session, what: &str, wanted: impl Fn(&Status) -> bool) -> Status {
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let status = session.status().expect("a status");
        if wanted(&status) {
            return status;
        }
        assert!(Instant::now() < deadline, "{what}: {status:?}");
        thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Whether `status` was heard where it says, no sooner than `sent`.
fn heard_since(status: &Status, sent: Instant) -> bool {
    status
        .at
        .inspect(|&at| {
            assert!(at >= sent, "heard there before it was sent: {status:?}");
        })
        .is_some()
}

/// A TV that starts playing somewhere new, saying so by event, isn't heard
/// playing where it was before.
#[test]
fn a_tv_that_plays_from_elsewhere_is_heard_playing_only_there() {
    use dlna::fake::{FakeRenderer, Kind as Renderer};
    let tv = FakeRenderer::start(Renderer::Tv);
    let mut session = session_with(&tv);
    let asked = tv.state().received("GetPositionInfo").len();
    session.load(&clip(0.0, false)).unwrap();
    assert!(tv.wait_for(|state| state.received("GetPositionInfo").len() > asked));
    {
        let mut state = tv.state();
        state.plays_after = Some(("Play".into(), 30.0));
        // Slow to say where, once it's said it plays.
        state.answer_after = std::time::Duration::from_millis(300);
    }
    session.play().unwrap();
    let playing = status_until(&mut *session, "playing", |status| {
        status.state == State::Playing
    });
    assert_eq!(playing.position, 30.0);
}

/// A TV that won't seek till it plays is sent there once it does: till
/// it's heard since, it's where it was sent, not where it was.
#[test]
fn a_tv_sent_somewhere_once_it_plays_is_there_till_heard_since() {
    use dlna::fake::{FakeRenderer, Kind as Renderer};
    let tv = FakeRenderer::start(Renderer::Tv);
    tv.state().refuse.push(("Seek".into(), 711));
    let mut session = session_with(&tv);
    session.load(&clip(1.0, true)).unwrap();
    assert!(tv.wait_for(|state| state.transport == "PLAYING"));
    let cleared = Instant::now();
    tv.state().refuse.clear();
    let heard = status_until(&mut *session, "heard there", |status| {
        assert_eq!(status.position, 1.0, "where it was sent: {status:?}");
        heard_since(status, cleared)
    });
    assert_eq!(heard.state, State::Playing);
    assert_eq!(tv.state().position, 1.0, "it went");
}

/// A TV sent somewhere, sought or given media, while it's being asked
/// where it is isn't heard where it was: the answer may be from before.
#[test]
fn a_tv_sent_somewhere_as_its_asked_where_it_is_is_where_it_was_sent() {
    use dlna::fake::{FakeRenderer, Kind as Renderer};
    for sought in [true, false] {
        let tv = FakeRenderer::start(Renderer::Tv);
        let mut session = session_with(&tv);
        session.load(&clip(0.0, true)).unwrap();
        status_until(&mut *session, "playing", |status| {
            status.state == State::Playing
        });
        let asked = {
            let mut state = tv.state();
            state.position = 5.0;
            state.answer_after = std::time::Duration::from_millis(300);
            state.received("GetPositionInfo").len()
        };
        assert!(tv.wait_for(|state| state.received("GetPositionInfo").len() > asked));
        let sent = Instant::now();
        let there = if sought {
            session.seek(20.0).unwrap();
            20.0
        } else {
            session.load(&clip(0.0, true)).unwrap();
            0.0
        };
        tv.state().answer_after = std::time::Duration::ZERO;
        status_until(&mut *session, "heard there, playing", |status| {
            assert_eq!(status.position, there, "where it was sent: {status:?}");
            heard_since(status, sent) && status.state == State::Playing
        });
    }
}

/// Subtitles changed once it's sought give it the media again from where
/// it was sought, before it's heard there.
#[test]
fn subtitles_changed_once_sought_go_from_there() {
    use dlna::fake::{FakeRenderer, Kind as Renderer};
    let tv = FakeRenderer::start(Renderer::Tv);
    let mut session = session_with(&tv);
    session.load(&clip(0.0, true)).unwrap();
    status_until(&mut *session, "playing", |status| {
        status.state == State::Playing
    });
    session.seek(20.0).unwrap();
    let subtitles = Subtitles {
        source: stream_server::SubtitleSource::Track(0),
        webvtt_url: "http://192.0.2.1:8080/cast/subs.vtt".into(),
        srt_url: "http://192.0.2.1:8080/cast/subs.srt".into(),
        language: None,
        name: "English".into(),
        style: crate::settings::SubtitleStyle::default(),
    };
    session.set_subtitles(Some(&subtitles)).unwrap();
    let state = tv.state();
    let given = state
        .calls
        .iter()
        .rposition(|call| call.action == "SetAVTransportURI")
        .unwrap();
    let seek = state.calls[given..]
        .iter()
        .find(|call| call.action == "Seek");
    assert_eq!(seek.and_then(|seek| seek.arg("Target")), Some("0:00:20"));
}
