use airplay::Features;
use airplay::fake::{FakeReceiver, Kind};

use super::*;
use crate::files::Folder;
use crate::media::Mixer;

fn speakers(fakes: &[&FakeReceiver]) -> Speakers {
    found(fakes.iter().map(|fake| fake.device()).collect())
}

/// Speakers that have found `receivers`.
fn found(receivers: Vec<airplay::Device>) -> Speakers {
    Speakers::with(receivers, Arc::new(Pairings::load(&Folder::app())))
}

fn output() -> AudioOutput {
    AudioOutput::with_mixer(Mixer::new(48_000, 1.0))
}

#[test]
fn receivers_that_play_sound_are_outputs_by_an_id_of_their_own() {
    let speaker = FakeReceiver::start(Kind::Speaker);
    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    let listed = speakers(&[&speaker, &tv]).list();
    let names: Vec<&str> = listed.iter().map(|output| output.name.as_str()).collect();
    assert_eq!(names, ["Fake Speaker", "Fake TV"], "a TV for its sound too");
    assert!(listed.iter().all(|output| Speakers::owns(&output.id)));
    assert_eq!(listed[0].id, format!("airplay:{}", speaker.device().id));
    assert!(!Speakers::owns("coreaudio:BuiltInSpeakerDevice"));
}

#[test]
fn a_receiver_that_only_shows_video_is_no_output() {
    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    let mut screen = tv.device();
    screen.features = Features(screen.features.0 & !(1 << Features::AUDIO));
    let speakers = found(vec![screen]);
    assert!(speakers.list().is_empty());
    let id = format!("airplay:{}", tv.device().id);
    let opened = speakers.open(&mut output(), &id, |_| {});
    assert!(matches!(opened, Err(OpenError::Missing)), "{opened:?}");
}

#[test]
fn a_speaker_is_told_its_own_volume_once_as_it_starts() {
    let speaker = FakeReceiver::start(Kind::Speaker);
    speaker.state().volume_db = -12.0;
    let speakers = speakers(&[&speaker]);
    let id = &speakers.list()[0].id;
    let mixer = Mixer::new(48_000, 1.0);
    let mut output = AudioOutput::with_mixer(Arc::clone(&mixer));
    let playing = speakers.open(&mut output, id, |_| {}).expect("playing");
    assert_eq!(playing.name, "Fake Speaker");
    assert_eq!(playing.id.as_deref(), Some(id.as_str()));
    assert_eq!(playing.rate, 44_100);
    assert_eq!(mixer.sample_rate(), 44_100, "the mixer mixes at its rate");
    let state = speaker.state();
    let set: Vec<String> = state
        .requests
        .iter()
        .filter(|request| request.method == "SET_PARAMETER")
        .map(|request| String::from_utf8_lossy(&request.body).trim().to_owned())
        .collect();
    assert_eq!(set, ["volume: -12.000000"], "where it was, once");
}

#[test]
fn one_not_found_is_missing_and_a_tv_wants_its_pin_until_paired() {
    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    let speakers = speakers(&[&tv]);
    let id = speakers.list()[0].id.clone();
    let mut output = output();
    let gone = speakers.open(&mut output, "airplay:00:00:00:00:00:00", |_| {});
    assert!(matches!(gone, Err(OpenError::Missing)), "{gone:?}");
    let unpaired = speakers.open(&mut output, &id, |_| {});
    assert!(matches!(unpaired, Err(OpenError::NeedsPin)), "{unpaired:?}");

    speakers
        .pair(&id)
        .and_then(|pairing| pairing.finish("1234"))
        .expect("paired");
    let playing = speakers.open(&mut output, &id, |_| {}).expect("playing");
    assert_eq!(playing.name, "Fake TV");
    assert!(tv.wait_for(|state| !state.packets.is_empty()));
}

#[test]
fn one_that_turns_this_computer_away_says_what_lets_it_in() {
    let speaker = FakeReceiver::start(Kind::Speaker);
    speaker.state().refuse_pairing = true;
    let speakers = speakers(&[&speaker]);
    let id = speakers.list()[0].id.clone();
    let Err(OpenError::Failed(why)) = speakers.open(&mut output(), &id, |_| {}) else {
        panic!("it doesn't play")
    };
    assert!(
        why.to_string()
            .starts_with("it lets only some devices play to it"),
        "{why}"
    );
}

#[test]
fn credentials_it_turns_down_ask_for_its_pin_again_and_are_kept_till_then() {
    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    let other = FakeReceiver::start(Kind::Tv { pin: "1234" });
    let speakers = speakers(&[&tv]);
    // Paired with another, as one reset since.
    let credentials = airplay::PinPairing::start(&other.device())
        .and_then(|pairing| pairing.finish("1234"))
        .expect("paired with the other");
    let device = tv.device();
    speakers
        .pairings
        .keep(&device.id, &device.name, &credentials);
    let id = speakers.list()[0].id.clone();
    let opened = speakers.open(&mut output(), &id, |_| {});
    assert!(matches!(opened, Err(OpenError::NeedsPin)), "{opened:?}");
    assert_eq!(
        speakers.pairings.credentials(&device.id),
        Some(credentials),
        "kept till it's paired again"
    );
}
