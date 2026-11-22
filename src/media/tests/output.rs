use std::sync::atomic::AtomicU64;
use std::sync::mpsc;

use airplay::Sender;
use airplay::fake::{FakeReceiver, Kind};

use super::*;
use crate::media::audio::{AudioChunk, AudioSource};
use crate::media::clock::Clock;

/// A player in `mixer` playing two seconds of `level` from `pts`, at
/// 48 kHz.
fn playing_in(mixer: &Mixer, level: f32, pts: f64) -> Arc<AudioSource> {
    let clock = Arc::new(Clock::new());
    clock.set(pts);
    clock.start();
    let source = Arc::new(AudioSource::new(
        clock,
        Arc::new(AtomicU64::new(0)),
        48_000,
        1.0,
    ));
    source.push(AudioChunk {
        pts,
        samples: vec![level; 96_000 * SOURCE_CHANNELS],
        pos: 0,
        rate: 48_000,
    });
    source.playing.store(true, Ordering::Relaxed);
    mixer.add_source(source.clone());
    source
}

/// `speaker`, connected to for sound.
fn connect(speaker: &FakeReceiver) -> (AudioStream, UnboundedReceiver<ReceiverEvent>) {
    AudioStream::connect(&speaker.device(), None, &Sender::new("Plays"))
        .expect("connected to the fake")
}

#[test]
fn a_speaker_plays_the_mix_and_what_it_says_it_adds_holds_the_clock_back() {
    let speaker = FakeReceiver::start(Kind::OldSpeaker);
    // A second of its own, on top of AirPlay's second and a half.
    speaker.state().audio_latency = 44_100;
    let (stream, events) = connect(&speaker);
    let mixer = Mixer::new(48_000, 1.0);
    let source = playing_in(&mixer, 0.25, 10.0);
    let mut output = AudioOutput::with_mixer(mixer.clone());
    let playing = output
        .play_to_speaker(stream, events, "airplay:old".into(), "Old".into(), |_| {})
        .cloned()
        .expect("playing through it");
    assert_eq!(playing.rate, 44_100);
    assert_eq!(mixer.sample_rate(), 44_100, "the speaker's rate");

    assert!(speaker.wait_for(|state| state.packets.len() >= 10));
    let heard = source.clock.now();
    // Sent from 10 s a moment ago, heard 2.5 s after it's sent.
    assert!(
        (heard - 7.5).abs() < 0.3,
        "the clock is at {heard}, not 2.5 s behind what's sent"
    );
    let samples = speaker.state().samples();
    let level = i16::from_sample(0.25f32);
    assert!(
        samples[..4000].iter().all(|&sample| sample == level),
        "the mix, as it is: {:?}",
        &samples[..8]
    );
    drop(output);
    assert!(
        speaker.wait_for(|state| state.closed == state.connections),
        "the stream ends with the output"
    );
    let state = speaker.state();
    assert!(
        state
            .requests
            .iter()
            .any(|request| request.method == "TEARDOWN"),
        "it's told the session ends"
    );
}

#[test]
fn a_speaker_going_away_is_heard_and_one_let_go_is_not() {
    let speaker = FakeReceiver::start(Kind::Speaker);
    let (stream, events) = connect(&speaker);
    let mut output = AudioOutput::with_mixer(Mixer::new(48_000, 1.0));
    let (told, heard) = mpsc::channel();
    output
        .play_to_speaker(
            stream,
            events,
            "airplay:s".into(),
            "S".into(),
            move |event| {
                let _ = told.send(event);
            },
        )
        .expect("playing through it");
    // On the event connection too, which the session's end comes by.
    assert!(speaker.wait_for(|state| { !state.packets.is_empty() && state.event_reply.is_some() }));
    speaker.end_session();
    assert_eq!(
        heard.recv_timeout(Duration::from_secs(5)),
        Ok(DeviceEvent::Lost)
    );

    let again = FakeReceiver::start(Kind::Speaker);
    let (stream, events) = connect(&again);
    let (told, heard) = mpsc::channel();
    output
        .play_to_speaker(
            stream,
            events,
            "airplay:s".into(),
            "S".into(),
            move |event| {
                let _ = told.send(event);
            },
        )
        .expect("playing through it");
    assert!(again.wait_for(|state| !state.packets.is_empty()));
    output.play_nowhere();
    assert!(output.playing().is_none());
    assert!(
        heard.recv_timeout(Duration::from_millis(500)).is_err(),
        "let go, it isn't lost"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn no_stream_latency_for_a_device_not_there() {
    assert_eq!(core_audio::stream_latency("no-such-device"), 0);
}

/// Needs the machine's own audio outputs: `cargo test -- --ignored`.
#[test]
#[ignore = "plays through this machine's audio outputs"]
fn plays_through_the_devices_here() {
    let devices = devices();
    eprintln!("outputs: {devices:#?}");
    let mixer = mixer(0.0);
    let mut output = AudioOutput::with_mixer(mixer.clone());
    let playing = output
        .play_through(None, |_| {})
        .cloned()
        .expect("a default output");
    eprintln!("default: {playing:?}");
    assert_eq!(mixer.sample_rate(), playing.rate);
    for device in &devices {
        assert!(is_connected(Some(&device.id)), "{device:?} by its id");
        let latency = find(Some(&device.id)).map(|device| stream_latency(&device));
        eprintln!("{}: {latency:?} frames of stream latency", device.name);
        match output.play_through(Some(&device.id), |_| {}).cloned() {
            Ok(playing) => {
                assert_eq!(playing.id.as_deref(), Some(device.id.as_str()));
                assert_eq!(mixer.sample_rate(), playing.rate);
            }
            Err(err) => eprintln!("{}: {err}", device.name),
        }
    }
    let gone = output.play_through(Some("coreaudio:no-such-device"), |_| {});
    assert!(matches!(gone, Err(OpenError::Missing)), "{gone:?}");
    assert!(output.playing().is_none());
}
