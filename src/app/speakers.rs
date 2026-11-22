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

/// The id among AirPlay receivers of output `id`, if it's an AirPlay
/// speaker.
pub fn airplay_id(id: &str) -> Option<&str> {
    id.strip_prefix(PREFIX)
}

/// The protocol output `id` is reached by: AirPlay for an AirPlay speaker,
/// none for this computer's own outputs. A device found by another
/// protocol too, as a speaker with DLNA, is told apart by it.
pub fn output_kind(id: &str) -> Option<&'static str> {
    airplay_id(id).map(|_| crate::cast::airplay::KIND)
}

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
        airplay_id(id).is_some()
    }

    /// The receiver output `id` is, as found now.
    fn receiver(&self, id: &str) -> Option<airplay::Device> {
        let id = airplay_id(id)?;
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
#[path = "tests/speakers.rs"]
mod tests;
