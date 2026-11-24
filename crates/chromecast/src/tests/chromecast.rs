//! Casts to the fake device: connecting and launching, the heartbeat,
//! LOAD and the commands after it, status, and every way a cast ends.

use std::thread;
use std::time::{Duration, Instant};

use futures_channel::mpsc::UnboundedReceiver;

use crate::fake::{FakeDevice, Kind};
use crate::proto::{CONNECTION, HEARTBEAT, MEDIA, PLATFORM, RECEIVER};
use crate::*;

/// The first event `wanted` picks, waited for up to `wait`.
fn wait_for_event(
    events: &mut UnboundedReceiver<Event>,
    wait: Duration,
    wanted: impl Fn(&Event) -> bool,
) -> Option<Event> {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        match events.try_recv() {
            Ok(event) if wanted(&event) => return Some(event),
            Ok(_) => {}
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
    None
}

fn ended(events: &mut UnboundedReceiver<Event>, wait: Duration) -> Option<Ended> {
    match wait_for_event(events, wait, |event| matches!(event, Event::Ended(_))) {
        Some(Event::Ended(why)) => Some(why),
        _ => None,
    }
}

fn film() -> Media {
    Media {
        url: "http://192.0.2.1:8080/cast/master.m3u8".into(),
        content_type: "application/x-mpegURL".into(),
        stream_type: StreamType::Buffered,
        title: Some("A Film".into()),
        duration: Some(5400.0),
        text_tracks: vec![TextTrack {
            id: 1,
            url: "http://192.0.2.1:8080/cast/subtitles.vtt".into(),
            content_type: "text/vtt".into(),
            name: "English".into(),
            language: Some("en".into()),
        }],
        hls_segments: Some(HlsSegments::Ts),
    }
}

/// A session to a fake TV with the film loaded.
fn casting() -> (FakeDevice, Session, UnboundedReceiver<Event>) {
    let fake = FakeDevice::start(Kind::Tv);
    let (session, events) = Session::connect(&fake.device()).unwrap();
    session.load(&film(), &Load::default()).unwrap();
    (fake, session, events)
}

#[test]
fn connecting_launches_the_default_media_receiver_and_keeps_the_heartbeat() {
    let fake = FakeDevice::start(Kind::Tv);
    let (session, mut events) = Session::connect(&fake.device()).unwrap();
    // CONNECT has no answer to wait for.
    assert!(fake.wait_for(|state| state.received(CONNECTION, "CONNECT").len() == 2));
    {
        let state = fake.state();
        let app = state.app.clone().expect("the app launched");
        let launch = state.received(RECEIVER, "LAUNCH");
        assert_eq!(launch[0].payload["appId"], "CC1AD845");
        let connects = state.received(CONNECTION, "CONNECT");
        let to: Vec<&str> = connects.iter().map(|c| c.destination.as_str()).collect();
        assert_eq!(to, [PLATFORM, app.transport.as_str()]);
    }
    assert!(
        fake.wait_for(|state| state.received(HEARTBEAT, "PING").len() >= 3 && state.pongs == 1),
        "it pings the device, and answers the device's ping"
    );
    // Well past the silence a cast ends after, the heartbeat keeps it.
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(ended(&mut events, Duration::ZERO), None);
    assert_eq!(session.status().unwrap(), None, "nothing loaded yet");
    assert_eq!(
        session.volume(),
        Some(Volume {
            level: 0.5,
            muted: false
        })
    );
}

#[test]
fn load_carries_the_url_where_to_start_and_the_styled_subtitles() {
    let fake = FakeDevice::start(Kind::Tv);
    let (session, mut events) = Session::connect(&fake.device()).unwrap();
    let style = TextTrackStyle {
        font_scale: 1.5,
        background: Color([0, 0, 0, 0x80]),
        edge: Edge::DropShadow,
        ..TextTrackStyle::default()
    };
    let status = session
        .load(
            &film(),
            &Load {
                start: 42.5,
                autoplay: true,
                active_tracks: vec![1],
                style: Some(style),
            },
        )
        .unwrap();
    assert_eq!(status.state, PlayerState::Playing);
    assert_eq!(status.position, 42.5);
    assert_eq!(status.duration, Some(5400.0));
    assert_eq!(status.active_tracks, [1]);

    let state = fake.state();
    let load = &state.received(MEDIA, "LOAD")[0].payload;
    assert_eq!(load["media"]["contentId"], film().url);
    assert_eq!(load["media"]["contentType"], "application/x-mpegURL");
    assert_eq!(load["currentTime"], 42.5);
    assert_eq!(load["activeTrackIds"], serde_json::json!([1]));
    let track = &load["media"]["tracks"][0];
    assert_eq!(
        track["trackContentId"],
        "http://192.0.2.1:8080/cast/subtitles.vtt"
    );
    assert_eq!(track["trackContentType"], "text/vtt");
    let style = state.playing.as_ref().unwrap().style.clone().unwrap();
    assert_eq!(style["fontScale"], 1.5);
    assert_eq!(style["backgroundColor"], "#00000080");
    assert_eq!(style["edgeType"], "DROP_SHADOW");
    drop(state);
    assert!(
        wait_for_event(&mut events, Duration::from_secs(1), |event| {
            matches!(event, Event::Media(status) if status.position == 42.5)
        })
        .is_some(),
        "its status comes as an event too"
    );
}

#[test]
fn play_pause_seek_subtitles_and_volume_reach_it() {
    let (fake, session, _events) = casting();
    assert_eq!(session.pause().unwrap().state, PlayerState::Paused);
    assert_eq!(session.play().unwrap().state, PlayerState::Playing);
    assert_eq!(session.seek(1234.0).unwrap().position, 1234.0);
    let style = TextTrackStyle {
        bold: true,
        ..TextTrackStyle::default()
    };
    assert_eq!(
        session
            .set_tracks(&[1], Some(&style))
            .unwrap()
            .active_tracks,
        [1]
    );
    assert!(
        session
            .set_tracks(&[], None)
            .unwrap()
            .active_tracks
            .is_empty()
    );
    assert_eq!(
        session.set_volume(0.25).unwrap(),
        Volume {
            level: 0.25,
            muted: false
        }
    );
    assert!(session.set_muted(true).unwrap().muted);
    assert_eq!(
        session.volume(),
        Some(Volume {
            level: 0.25,
            muted: true
        })
    );
    session.set_stream_volume(0.4, false).unwrap();

    let state = fake.state();
    let playing = state.playing.as_ref().unwrap();
    assert_eq!(playing.position, 1234.0);
    assert_eq!(
        (playing.volume, playing.muted),
        (0.4f32.into(), false),
        "what it plays has a volume of its own"
    );
    assert_eq!(state.volume, 0.25, "the device's stays");
    let stream = &state.received(MEDIA, "SET_VOLUME")[0];
    assert_eq!(stream.payload["mediaSessionId"], playing.media_session);
    assert_eq!(playing.style.as_ref().unwrap()["fontStyle"], "BOLD");
    assert!(state.muted);
    let pause = &state.received(MEDIA, "PAUSE")[0];
    assert_eq!(pause.payload["mediaSessionId"], playing.media_session);
    assert_eq!(pause.destination, state.app.as_ref().unwrap().transport);
}

#[test]
fn what_the_device_says_comes_as_events() {
    let (fake, session, mut events) = casting();
    fake.finish();
    let finished = wait_for_event(
        &mut events,
        Duration::from_secs(1),
        |event| matches!(event, Event::Media(status) if status.state == PlayerState::Idle(Some(IdleReason::Finished))),
    );
    let Some(Event::Media(status)) = finished else {
        panic!("the end comes as an event")
    };
    assert_eq!(status.position, 5400.0);
    session.set_volume(0.75).unwrap();
    assert!(
        wait_for_event(&mut events, Duration::from_secs(1), |event| {
            *event
                == Event::Volume(Volume {
                    level: 0.75,
                    muted: false,
                })
        })
        .is_some()
    );
}

#[test]
fn a_refused_load_says_so() {
    let fake = FakeDevice::start(Kind::Tv);
    fake.state().refuse_load = true;
    let (session, _events) = Session::connect(&fake.device()).unwrap();
    assert!(matches!(
        session.load(&film(), &Load::default()),
        Err(Error::Refused { request, .. }) if request == "LOAD"
    ));
    assert!(
        matches!(session.play(), Err(Error::Protocol(_))),
        "nothing loaded"
    );
}

#[test]
fn the_cast_ends_when_the_device_goes_away() {
    let (fake, session, mut events) = casting();
    fake.go_away();
    assert_eq!(
        ended(&mut events, Duration::from_secs(5)),
        Some(Ended::Gone)
    );
    assert!(matches!(session.play(), Err(Error::Ended)));
}

#[test]
fn the_cast_ends_when_the_device_stops_answering() {
    let (fake, _session, mut events) = casting();
    fake.state().silent = true;
    assert_eq!(
        ended(&mut events, Duration::from_secs(5)),
        Some(Ended::Gone)
    );
}

#[test]
fn the_cast_ends_when_the_app_closes() {
    let (fake, _session, mut events) = casting();
    fake.close_app();
    assert_eq!(
        ended(&mut events, Duration::from_secs(1)),
        Some(Ended::Stopped)
    );
}

#[test]
fn the_cast_ends_when_another_app_launches() {
    let (fake, _session, mut events) = casting();
    fake.launch_another_app();
    assert_eq!(
        ended(&mut events, Duration::from_secs(1)),
        Some(Ended::Stopped)
    );
}

#[test]
fn the_cast_ends_when_another_sender_plays_something_else() {
    let (fake, _session, mut events) = casting();
    fake.replace();
    assert_eq!(
        ended(&mut events, Duration::from_secs(1)),
        Some(Ended::Replaced)
    );
}

#[test]
fn loading_again_isnt_taken_for_another_sender() {
    let (_fake, session, mut events) = casting();
    let again = session.load(&film(), &Load::default()).unwrap();
    assert_eq!(session.pause().unwrap().media_session, again.media_session);
    assert_eq!(ended(&mut events, Duration::from_millis(200)), None);
}

#[test]
fn stopping_closes_the_app_without_news() {
    let (fake, session, mut events) = casting();
    let app = fake.state().app.clone().unwrap();
    session.stop();
    let state = fake.state();
    assert!(state.app.is_none());
    assert_eq!(
        state.received(RECEIVER, "STOP")[0].payload["sessionId"],
        app.session
    );
    drop(state);
    assert_eq!(ended(&mut events, Duration::from_millis(200)), None);
}

#[test]
fn a_speaker_takes_a_cast_too() {
    let fake = FakeDevice::start(Kind::Speaker);
    let device = fake.device();
    assert!(!device.shows_video());
    let (session, _events) = Session::connect(&device).unwrap();
    let song = Media {
        url: "http://192.0.2.1:8080/cast/song.flac".into(),
        content_type: "audio/flac".into(),
        stream_type: StreamType::Buffered,
        title: None,
        duration: None,
        text_tracks: Vec::new(),
        hls_segments: None,
    };
    assert_eq!(
        session.load(&song, &Load::default()).unwrap().state,
        PlayerState::Playing
    );
}
