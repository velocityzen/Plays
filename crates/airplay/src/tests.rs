//! Sessions against the fake receiver: pairing, a TV playing a URL, and
//! sound to both kinds of speaker.

use std::thread;
use std::time::{Duration, Instant};

use futures_channel::mpsc::UnboundedReceiver;

use crate::audio::LATENCY;
use crate::fake::{FakeReceiver, Kind};
use crate::rtp::{CHANNELS, FRAMES_PER_PACKET};
use crate::*;

const PIN: &str = "4719";

fn sender() -> Sender {
    Sender {
        name: "Plays test".into(),
    }
}

/// The next event, waited for up to `wait`.
fn next_event(events: &mut UnboundedReceiver<Event>, wait: Duration) -> Option<Event> {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if let Ok(event) = events.try_recv() {
            return Some(event);
        }
        thread::sleep(Duration::from_millis(20));
    }
    None
}

/// Stereo samples, no two frames alike.
fn sound(frames: usize) -> Vec<i16> {
    (0..frames)
        .flat_map(|i| {
            let sample = (i % 20_000) as i16 - 10_000;
            [sample, -sample]
        })
        .collect()
}

fn pair(fake: &FakeReceiver) -> Credentials {
    let pairing = PinPairing::start(&fake.device()).unwrap();
    assert!(fake.state().pin_shown, "the TV shows its PIN");
    pairing.finish(PIN).unwrap()
}

#[test]
fn a_tv_pairs_once_with_its_pin_and_is_verified_after() {
    let fake = FakeReceiver::start(Kind::Tv { pin: PIN });
    let device = fake.device();
    assert_eq!(device.pairing(), Pairing::Pin);
    assert!(matches!(
        VideoSession::connect(&device, None, &sender()),
        Err(Error::NeedsPin)
    ));

    let credentials = pair(&fake);
    assert_eq!(fake.paired(), std::slice::from_ref(&credentials.id));
    // What pairing leaves is kept as text, and does for every session.
    let kept = Credentials::from_text(&credentials.to_text()).unwrap();
    for _ in 0..2 {
        let (session, _events) = VideoSession::connect(&device, Some(&kept), &sender()).unwrap();
        session.stop();
    }
    assert_eq!(fake.paired().len(), 1);
    assert_eq!(fake.state().requests("POST", "/pair-verify").len(), 4);
}

#[test]
fn a_wrong_pin_says_so_and_pairs_nothing() {
    let fake = FakeReceiver::start(Kind::Tv { pin: PIN });
    let pairing = PinPairing::start(&fake.device()).unwrap();
    let Err(Error::Pairing(message)) = pairing.finish("0000") else {
        panic!("a wrong PIN doesn't pair")
    };
    assert!(message.contains("PIN"), "{message}");
    assert!(fake.paired().is_empty());
}

#[test]
fn credentials_for_another_tv_dont_verify() {
    let one = FakeReceiver::start(Kind::Tv { pin: PIN });
    let other = FakeReceiver::start(Kind::Tv { pin: PIN });
    let credentials = pair(&one);
    assert!(matches!(
        VideoSession::connect(&other.device(), Some(&credentials), &sender()),
        Err(Error::Pairing(_))
    ));
}

#[test]
fn a_tv_plays_a_url_and_is_driven_while_it_does() {
    let fake = FakeReceiver::start(Kind::Tv { pin: PIN });
    let credentials = pair(&fake);
    let (mut session, _events) =
        VideoSession::connect(&fake.device(), Some(&credentials), &sender()).unwrap();
    let url = "http://192.0.2.1:8080/stream/master.m3u8";
    session.play(url, 12.0, true).unwrap();
    assert_eq!(
        session.playback_info().unwrap(),
        PlaybackInfo {
            state: PlaybackState::Playing,
            position: 12.0,
            duration: Some(60.0),
        }
    );
    session.pause().unwrap();
    assert_eq!(
        session.playback_info().unwrap().state,
        PlaybackState::Paused
    );
    session.seek(30.5).unwrap();
    assert_eq!(session.playback_info().unwrap().position, 30.5);
    session.resume().unwrap();
    session.set_volume(0.8, false).unwrap();
    assert!(
        fake.wait_for(|state| state.event_reply == Some(200)),
        "it answers the TV"
    );
    {
        let state = fake.state();
        let play = state.requests("POST", "/play")[0];
        assert!(play.sealed, "it's said encrypted");
        let body = play.dictionary();
        assert_eq!(body["Content-Location"].as_string(), Some(url));
        assert_eq!(body["Start-Position-Seconds"].as_real(), Some(12.0));
        assert!(play.header("X-Apple-Session-ID").is_some());
        let rates: Vec<_> = state
            .requests("POST", "/rate")
            .iter()
            .map(|rate| rate.query("value"))
            .collect();
        assert_eq!(rates, [Some(1.0), Some(0.0), Some(1.0)]);
        assert_eq!(
            state.requests("POST", "/scrub")[0].query("position"),
            Some(30.5)
        );
        assert_eq!(state.volume_db, -6.0);
    }

    // The TV saying nothing once it played is the end.
    fake.state().playback.clear();
    assert_eq!(session.playback_info().unwrap().state, PlaybackState::Ended);
    session.stop();
    let state = fake.state();
    assert_eq!(state.requests("POST", "/stop").len(), 1);
    assert!(
        state
            .requests
            .iter()
            .any(|request| request.method == "TEARDOWN")
    );
}

#[test]
fn a_speaker_doesnt_take_a_url() {
    let fake = FakeReceiver::start(Kind::Speaker);
    assert!(matches!(
        VideoSession::connect(&fake.device(), None, &sender()),
        Err(Error::Protocol(_))
    ));
}

#[test]
fn a_speaker_gets_the_sound_encrypted_and_in_time() {
    let fake = FakeReceiver::start(Kind::Speaker);
    let device = fake.device();
    assert_eq!(device.pairing(), Pairing::Transient);
    let (mut stream, _events) = AudioStream::connect(&device, None, &sender()).unwrap();
    assert_eq!(stream.latency(), Duration::from_millis(1500));

    // Ten packets and a bit: the bit waits for more.
    let sound = sound(10 * FRAMES_PER_PACKET + 100);
    let started = Instant::now();
    stream.write(&sound).unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(70),
        "sent as due, not at once"
    );
    assert_eq!(stream.frames(), sound.len() as u64 / 2);
    let heard = stream.heard_at(0).unwrap() - started;
    assert!(heard.abs_diff(stream.latency()) < Duration::from_millis(20));

    assert!(fake.wait_for(|state| state.packets.len() == 10 && state.event_reply.is_some()));
    let state = fake.state();
    assert_eq!(state.samples(), sound[..10 * FRAMES_PER_PACKET * CHANNELS]);
    assert_eq!(state.packets.keys().next(), Some(&LATENCY));
    assert_eq!(
        state.syncs.first().map(|sync| (sync.0, sync.2)),
        Some((0, LATENCY))
    );
    let (asked, answer) = state.timing.clone().expect("it asked the time");
    assert_eq!(answer[8..16], asked.to_be_bytes(), "our clock answers");
    assert_eq!(state.event_reply, Some(200));
    assert!(
        state
            .requests
            .iter()
            .all(|request| request.sealed || request.path() == "/pair-setup")
    );
}

#[test]
fn a_speaker_missing_packets_gets_them_again() {
    let fake = FakeReceiver::start(Kind::Speaker);
    let (mut stream, _events) = AudioStream::connect(&fake.device(), None, &sender()).unwrap();
    stream.write(&sound(4 * FRAMES_PER_PACKET)).unwrap();
    assert!(fake.wait_for(|state| state.packets.len() == 4));
    let seqs: Vec<u16> = fake
        .state()
        .packets
        .values()
        .map(|packet| packet.seq)
        .collect();
    fake.ask_again(seqs[1], 2);
    assert!(fake.wait_for(|state| state.resent == seqs[1..3]));
}

#[test]
fn a_speakers_volume_and_a_flush() {
    let fake = FakeReceiver::start(Kind::Speaker);
    let (mut stream, _events) = AudioStream::connect(&fake.device(), None, &sender()).unwrap();
    stream.set_volume(0.8, false).unwrap();
    assert_eq!(fake.state().volume_db, -6.0);
    assert!((stream.volume().unwrap() - 0.8).abs() < 1e-4);
    stream.set_volume(0.8, true).unwrap();
    assert_eq!(fake.state().volume_db, -144.0);

    stream.write(&sound(2 * FRAMES_PER_PACKET + 5)).unwrap();
    stream.flush().unwrap();
    assert_eq!(stream.frames(), 0);
    let state = fake.state();
    let flush = state
        .requests
        .iter()
        .find(|request| request.method == "FLUSH")
        .expect("a FLUSH");
    let next = LATENCY + 2 * FRAMES_PER_PACKET as u32;
    assert!(
        flush
            .header("RTP-Info")
            .is_some_and(|info| info.ends_with(&format!("rtptime={next}")))
    );
}

#[test]
fn a_receiver_going_away_ends_the_session_at_once() {
    let fake = FakeReceiver::start(Kind::Speaker);
    let (stream, mut events) = AudioStream::connect(&fake.device(), None, &sender()).unwrap();
    assert!(fake.wait_for(|state| state.event_reply.is_some()));
    fake.go_away();
    // Its event connection closing says so, before feedback is due.
    assert_eq!(
        next_event(&mut events, Duration::from_secs(1)),
        Some(Event::Gone)
    );
    stream.stop();
}

#[test]
fn a_receiver_refusing_feedback_ends_the_session() {
    let fake = FakeReceiver::start(Kind::Tv { pin: PIN });
    let credentials = pair(&fake);
    let (session, mut events) =
        VideoSession::connect(&fake.device(), Some(&credentials), &sender()).unwrap();
    fake.state().refuse_feedback = true;
    assert_eq!(
        next_event(&mut events, Duration::from_secs(3)),
        Some(Event::Gone)
    );
    assert_eq!(fake.state().requests("POST", "/feedback").len(), 1);
    session.stop();
}

#[test]
fn an_airplay_1_speaker_gets_plain_sound_and_says_its_delay() {
    let fake = FakeReceiver::start(Kind::OldSpeaker);
    fake.state().audio_latency = 11_025;
    let device = fake.device();
    assert!(!device.airplay2());
    assert_eq!(device.pairing(), Pairing::None);
    let (mut stream, _events) = AudioStream::connect(&device, None, &sender()).unwrap();
    assert_eq!(stream.latency(), Duration::from_millis(1750));

    let sound = sound(3 * FRAMES_PER_PACKET);
    stream.write(&sound).unwrap();
    assert!(fake.wait_for(|state| state.packets.len() == 3 && !state.syncs.is_empty()));
    let state = fake.state();
    assert_eq!(state.samples(), sound);
    assert!(state.requests.iter().all(|request| !request.sealed));
    let announce = &state.requests[0];
    assert_eq!(announce.method, "ANNOUNCE");
    assert!(String::from_utf8_lossy(&announce.body).contains("L16/44100/2"));
    let record = state
        .requests
        .iter()
        .find(|request| request.method == "RECORD")
        .expect("a RECORD");
    assert_eq!(record.header("Session"), Some("1"));
}

#[test]
fn an_airplay_1_speaker_that_takes_only_encrypted_sound_is_refused() {
    let fake = FakeReceiver::start(Kind::OldSpeaker);
    let mut device = fake.device();
    device.encryptions = vec![1];
    assert!(matches!(
        AudioStream::connect(&device, None, &sender()),
        Err(Error::Protocol(_))
    ));
    assert!(fake.state().requests.is_empty(), "without a word to it");
}
