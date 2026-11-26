use super::*;

#[test]
fn a_receiver_that_turns_this_computer_away_says_what_lets_it_in() {
    let fake = ::airplay::fake::FakeReceiver::start(::airplay::fake::Kind::Speaker);
    let says = |model: &str| {
        let mut receiver = fake.device();
        receiver.model = model.into();
        match failed_at(&receiver, &::airplay::Error::Refused) {
            Error::Failed(why) => why,
            other => panic!("{other}"),
        }
    };
    for mac in [
        "Mac13,2",
        "Mac16,5",
        "MacBookPro18,3",
        "iMac21,1",
        "Macmini9,1",
    ] {
        let why = says(mac);
        assert!(why.starts_with("it lets only some devices play to it. "));
        assert!(
            why.contains("AirDrop & Handoff → Allow AirPlay for"),
            "{mac}"
        );
    }
    for home in ["AppleTV11,1", "AudioAccessory5,1"] {
        assert!(says(home).contains("Home app"), "{home}");
    }
    assert!(says("Phantom").ends_with("Its settings say which."));
    assert!(matches!(
        failed_at(&fake.device(), &::airplay::Error::NeedsPin),
        Error::Failed(why) if why.contains("PIN")
    ));
}

#[test]
fn a_tv_that_turns_this_computer_away_later_says_what_lets_it_in() {
    use ::airplay::fake::{FakeReceiver, Kind};

    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    let pairings = Arc::new(Pairings::load(&crate::files::Folder::app()));
    let target = AirPlayTarget::with(vec![tv.device()], Arc::clone(&pairings));
    let turned_away = |result: Result<(), Error>| match result {
        Err(Error::Failed(why)) => why.starts_with("it lets only some devices play to it"),
        _ => false,
    };
    // Once it has shown its PIN.
    let pairing = AirPlayPairing::start(tv.device(), Arc::clone(&pairings)).unwrap();
    tv.state().refuse_pairing = true;
    assert!(turned_away(pairing.finish("1234")));

    // Paired with before.
    tv.state().refuse_pairing = false;
    AirPlayPairing::start(tv.device(), Arc::clone(&pairings))
        .and_then(|pairing| pairing.finish("1234"))
        .unwrap();
    tv.state().refuse_pairing = true;
    let device = target.devices().remove(0);
    assert!(turned_away(target.connect(&device).map(drop)));
}

#[test]
fn an_apple_tv_4k_gets_hevc_and_hdr_up_to_2160p_in_fragmented_mp4() {
    for model in ["AppleTV6,2", "AppleTV11,1", "AppleTV14,1"] {
        let tv = profile(model, None);
        assert_eq!(tv.video, [VideoCodec::H264, VideoCodec::Hevc], "{model}");
        assert_eq!(tv.max_size, Some(UHD), "{model}");
        assert_eq!(tv.max_bit_depth, 10, "{model}");
        assert_eq!(tv.hdr, [Hdr::Hdr10, Hdr::Hlg, Hdr::DolbyVision], "{model}");
        assert_eq!(tv.hls, Some(Segments::Fmp4), "{model}");
        assert_eq!(
            tv.audio,
            [
                AudioCodec::Aac,
                AudioCodec::Ac3,
                AudioCodec::Eac3,
                AudioCodec::Alac
            ],
            "{model}"
        );
        assert_eq!(tv.max_channels, 8, "{model}");
    }
}

#[test]
fn an_apple_tv_hd_gets_hevc_without_hdr_up_to_1080p() {
    let hd = profile("AppleTV5,3", None);
    assert_eq!(hd.video, [VideoCodec::H264, VideoCodec::Hevc]);
    assert_eq!(hd.max_size, Some(FULL_HD));
    assert_eq!(hd.max_bit_depth, 10);
    assert!(hd.hdr.is_empty());
    assert_eq!(hd.hls, Some(Segments::Fmp4));
    assert_eq!(hd.max_channels, 8);
}

#[test]
fn an_older_apple_tv_and_any_other_tv_get_h264() {
    let old = profile("AppleTV3,2", None);
    assert_eq!(old.video, [VideoCodec::H264]);
    assert_eq!(old.max_size, Some(FULL_HD));
    assert_eq!(old.hls, Some(Segments::Ts));
    assert!(old.audio.contains(&AudioCodec::Ac3));
    for model in ["UN55TU8000", "AppleTV", "", "AppleTVx,1"] {
        let other = profile(model, None);
        assert_eq!(other.video, [VideoCodec::H264], "{model}");
        assert_eq!(other.audio, [AudioCodec::Aac], "{model}");
        assert_eq!(other.max_channels, 2, "{model}");
        assert_eq!(other.hls, Some(Segments::Ts), "{model}");
    }
}

/// What a Google TV projector with AirPlay built in says its screen shows.
fn projector() -> Screen {
    Screen {
        max_size: Some(UHD),
        hdr: vec![
            ::airplay::Hdr::Hdr10,
            ::airplay::Hdr::DolbyVision,
            ::airplay::Hdr::Hlg,
        ],
    }
}

#[test]
fn a_tv_of_another_make_gets_hevc_with_the_hdr_its_screen_shows_up_to_its_largest_picture() {
    let tv = profile("VisionMaster", Some(&projector()));
    assert_eq!(tv.video, [VideoCodec::H264, VideoCodec::Hevc]);
    assert_eq!(tv.max_size, Some(UHD));
    assert_eq!(tv.max_bit_depth, 10);
    assert_eq!(tv.hdr, [Hdr::Hdr10, Hdr::DolbyVision, Hdr::Hlg]);
    assert_eq!(tv.hls, Some(Segments::Fmp4));
    assert_eq!(
        tv.audio,
        [AudioCodec::Aac],
        "the sound nearly every TV plays"
    );
    assert_eq!(tv.max_channels, 2);
    let no_size = Screen {
        max_size: None,
        ..projector()
    };
    assert_eq!(
        profile("VisionMaster", Some(&no_size)).max_size,
        Some(FULL_HD)
    );
}

#[test]
fn a_screen_that_shows_no_hdr_gets_h264_and_an_apple_tv_goes_by_its_model() {
    let sdr = Screen {
        max_size: Some(UHD),
        hdr: Vec::new(),
    };
    assert_eq!(
        profile("UN55TU8000", Some(&sdr)),
        profile("UN55TU8000", None)
    );
    for model in ["AppleTV11,1", "AppleTV5,3", "AppleTV3,2"] {
        assert_eq!(
            profile(model, Some(&projector())),
            profile(model, None),
            "{model}"
        );
    }
}

#[test]
fn a_tv_of_another_make_is_asked_what_its_screen_shows_and_an_apple_tv_isnt() {
    use ::airplay::fake::{FakeReceiver, Kind};
    use plist::{Dictionary, Value};

    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    let mode = |name: &str| {
        let mut mode = Dictionary::new();
        mode.insert("HDRMode".into(), name.into());
        Value::Dictionary(mode)
    };
    let mut display = Dictionary::new();
    display.insert("widthPixelsMax".into(), Value::Integer(3840.into()));
    display.insert("heightPixelsMax".into(), Value::Integer(2160.into()));
    display.insert(
        "HDRSupportedModes".into(),
        Value::Array(vec![mode("SDR"), mode("HDR10"), mode("DoVi"), mode("HLG")]),
    );
    let mut info = Dictionary::new();
    info.insert(
        "displays".into(),
        Value::Array(vec![Value::Dictionary(display)]),
    );
    tv.state().info = Some(info);
    let pairings = Arc::new(Pairings::load(&crate::files::Folder::app()));
    AirPlayPairing::start(tv.device(), Arc::clone(&pairings))
        .and_then(|pairing| pairing.finish("1234"))
        .unwrap();

    let apple = AirPlayTarget::with(vec![tv.device()], Arc::clone(&pairings));
    let mut session = apple.connect(&apple.devices().remove(0)).unwrap();
    assert_eq!(session.profile(), profile("AppleTV11,1", None));
    assert!(tv.state().requests("GET", "/info").is_empty());
    session.stop().unwrap();

    let mut projector = tv.device();
    projector.model = "VisionMaster".into();
    let other = AirPlayTarget::with(vec![projector], pairings);
    let mut session = other.connect(&other.devices().remove(0)).unwrap();
    let served = session.profile();
    assert_eq!(served.hdr, [Hdr::Hdr10, Hdr::DolbyVision, Hdr::Hlg]);
    assert_eq!(served.max_size, Some(UHD));
    assert_eq!(served.video, [VideoCodec::H264, VideoCodec::Hevc]);
    assert_eq!(tv.state().requests("GET", "/info").len(), 1);
    session.stop().unwrap();
}

#[test]
fn everything_goes_as_hls_with_subtitles_in_the_playlist() {
    for model in ["AppleTV11,1", "AppleTV5,3", "AppleTV3,2", "UN55TU8000"] {
        let tv = profile(model, None);
        assert!(tv.files.is_empty(), "{model}");
        assert!(tv.hls_subtitles, "{model}");
        assert!(tv.fetches_urls, "{model}");
        assert!(!tv.stream && tv.sound_stream.is_empty(), "{model}");
    }
}

#[test]
fn what_it_says_its_doing_is_what_the_block_shows() {
    assert_eq!(state(PlaybackState::Loading), State::Buffering);
    assert_eq!(state(PlaybackState::Playing), State::Playing);
    assert_eq!(state(PlaybackState::Paused), State::Paused);
    assert_eq!(state(PlaybackState::Ended), State::Ended);
}
