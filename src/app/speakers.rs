//! AirPlay speakers as outputs of Plays' mixer, beside this computer's own:
//! found on the network, listed with the other outputs as they come and
//! go, and played through with the mixer's sound, straight from it, while
//! the pictures stay here, in step. Apple TVs are listed too, for their
//! sound. One that wants it is paired with the PIN it shows, the first
//! time, as a TV cast to is: what that leaves is kept in the same
//! `Pairings`.

use std::sync::Arc;
use std::thread;

use airplay::{AudioStream, Browser, Found};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use parking_lot::Mutex;

use crate::cast::Pairings;
use crate::cast::airplay::{AirPlayPairing, turned_away};
use crate::cast::target::{Error, Pairing};
use crate::media::AudioOutput;
use crate::media::output::{DeviceEvent, DeviceInfo, OpenError, Playing};

/// What a speaker's id starts with among the outputs, after which comes
/// its id among AirPlay receivers.
const PREFIX: &str = "airplay:";

pub struct Speakers {
    /// Looking for them until dropped; tests have their fakes instead.
    browser: Option<Browser>,
    /// What the browser finds, until watched.
    found: Mutex<Option<UnboundedReceiver<Found>>>,
    pairings: Arc<Pairings>,
    #[cfg(test)]
    fakes: Mutex<Vec<airplay::Device>>,
}

impl Speakers {
    /// Look for them on every network this computer is on, connecting as
    /// `pairings` say; `None` if it can't look.
    pub fn start(pairings: Arc<Pairings>) -> Option<Self> {
        match Browser::start() {
            Ok((browser, found)) => Some(Self {
                browser: Some(browser),
                found: Mutex::new(Some(found)),
                pairings,
                #[cfg(test)]
                fakes: Mutex::default(),
            }),
            Err(err) => {
                log::warn!("can't look for AirPlay speakers: {err}");
                None
            }
        }
    }

    /// For tests: these fake receivers, as if found.
    #[cfg(test)]
    pub fn with(fakes: Vec<airplay::Device>, pairings: Arc<Pairings>) -> Self {
        Self {
            browser: None,
            found: Mutex::default(),
            pairings,
            fakes: Mutex::new(fakes),
        }
    }

    /// For tests: these are found now instead.
    #[cfg(test)]
    pub fn set_fakes(&self, fakes: Vec<airplay::Device>) {
        *self.fakes.lock() = fakes;
    }

    /// Call `changed` whenever one is found, changes or goes, from a thread
    /// of its own, as long as it looks.
    pub fn watch(&self, changed: Box<dyn Fn() + Send + Sync>) {
        let Some(mut found) = self.found.lock().take() else {
            return;
        };
        thread::Builder::new()
            .name("airplay speakers".into())
            .spawn(move || {
                while futures::executor::block_on(found.next()).is_some() {
                    changed();
                }
            })
            .inspect_err(|err| log::warn!("can't follow AirPlay speakers: {err}"))
            .ok();
    }

    fn found(&self) -> Vec<airplay::Device> {
        let found = self
            .browser
            .as_ref()
            .map(Browser::devices)
            .unwrap_or_default();
        #[cfg(test)]
        let found = [found, self.fakes.lock().clone()].concat();
        found
    }

    /// The receivers that play sound, as outputs.
    pub fn list(&self) -> Vec<DeviceInfo> {
        self.found()
            .into_iter()
            .filter(airplay::Device::plays_audio)
            .map(|receiver| DeviceInfo {
                id: format!("{PREFIX}{}", receiver.id),
                name: receiver.name,
            })
            .collect()
    }

    /// Whether output `id` is one of these.
    pub fn owns(id: &str) -> bool {
        id.starts_with(PREFIX)
    }

    /// The receiver output `id` is, as found now.
    fn receiver(&self, id: &str) -> Option<airplay::Device> {
        let id = id.strip_prefix(PREFIX)?;
        self.found()
            .into_iter()
            .find(|receiver| receiver.id == id && receiver.plays_audio())
    }

    /// Whether output `id` is found now.
    pub fn is_there(&self, id: &str) -> bool {
        self.receiver(id).is_some()
    }

    /// Play `output` through speaker `id`, connected with what pairing
    /// with it left, if it was. Its own volume is set once, to where it
    /// says it is, as senders tell it as they start; the master volume and
    /// each player's set the level from then on. Waits on the network.
    pub fn open(
        &self,
        output: &mut AudioOutput,
        id: &str,
        on_event: impl Fn(DeviceEvent) + Send + 'static,
    ) -> Result<Playing, OpenError> {
        let receiver = self.receiver(id).ok_or(OpenError::Missing)?;
        let credentials = self.pairings.credentials(&receiver.id);
        let sender = self.pairings.sender();
        let (mut stream, events) =
            match AudioStream::connect(&receiver, credentials.as_ref(), &sender) {
                Ok(connected) => connected,
                Err(airplay::Error::NeedsPin) => return Err(OpenError::NeedsPin),
                // It no longer knows us, as after it's reset: paired again.
                Err(airplay::Error::Pairing(why)) if credentials.is_some() => {
                    log::info!("airplay: {} turned down its pairing: {why}", receiver.name);
                    self.pairings.forget(&receiver.id);
                    return Err(OpenError::NeedsPin);
                }
                Err(airplay::Error::Refused) => {
                    return Err(OpenError::Failed(anyhow::anyhow!(turned_away(&receiver))));
                }
                Err(err) => return Err(OpenError::Failed(anyhow::Error::new(err))),
            };
        match stream.volume() {
            Ok(volume) => {
                if let Err(err) = stream.set_volume(volume, false) {
                    log::info!(
                        "airplay: {} kept its volume to itself: {err}",
                        receiver.name
                    );
                }
            }
            Err(err) => log::info!("airplay: {} didn't say its volume: {err}", receiver.name),
        }
        output
            .play_to_speaker(stream, events, id.to_owned(), receiver.name, on_event)
            .cloned()
    }

    /// Start pairing with speaker `id`, which said it needs its PIN: it
    /// shows one. Waits on the network.
    pub fn pair(&self, id: &str) -> Result<Box<dyn Pairing>, Error> {
        let receiver = self.receiver(id).ok_or(Error::Gone)?;
        AirPlayPairing::start(receiver, Arc::clone(&self.pairings))
    }
}

#[cfg(test)]
mod tests {
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
    fn credentials_it_turns_down_are_forgotten_and_its_pin_asked_for_again() {
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
        assert!(speakers.pairings.paired().is_empty(), "forgotten");
    }
}
