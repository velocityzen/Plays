//! AirPlay receivers that show video, through the `airplay` crate: Apple
//! TVs, and TVs with AirPlay built in. They're told to play a URL, as
//! iPhones tell them, and driven while they do. One that wants it is paired
//! with the PIN it shows, the first time: what that leaves is kept
//! (`Pairings`), and verified on each connection after.
//!
//! What one plays goes by its model: an Apple TV 4K HEVC and HDR, as Apple
//! lists. Everything goes as HLS, so subtitles go in its playlist, which
//! the receiver draws in its own style; other subtitles have it play the
//! media again from where it is, as it reads the playlist only once.

use std::sync::Arc;

use ::airplay::{Browser, Event, PinPairing, PlaybackInfo, PlaybackState, VideoSession};
use futures::channel::mpsc::UnboundedReceiver;
use stream_server::{AudioCodec, Hdr, Profile, Segments, VideoCodec};

use super::pairings::Pairings;
use super::target::{
    Device, Error, Load, Pairing, Session, Shows, State, Status, Subtitles, Target,
};

pub const KIND: &str = "AirPlay";

const FULL_HD: (u32, u32) = (1920, 1080);
const UHD: (u32, u32) = (3840, 2160);

pub struct AirPlayTarget {
    /// Looking for receivers until dropped; tests have their fakes instead.
    browser: Option<Browser>,
    pairings: Arc<Pairings>,
    #[cfg(test)]
    fakes: Vec<::airplay::Device>,
}

impl AirPlayTarget {
    /// Look for receivers on every network this computer is on, connecting
    /// to them as `pairings` say.
    pub fn start(pairings: Arc<Pairings>) -> ::airplay::Result<Self> {
        // What's found is asked for when wanted, so news of it goes unheard.
        let (browser, _found) = Browser::start()?;
        Ok(Self {
            browser: Some(browser),
            pairings,
            #[cfg(test)]
            fakes: Vec::new(),
        })
    }

    /// For tests: these fake receivers, as if found.
    #[cfg(test)]
    pub fn with(fakes: Vec<::airplay::Device>, pairings: Arc<Pairings>) -> Self {
        Self {
            browser: None,
            pairings,
            fakes,
        }
    }

    fn found(&self) -> Vec<::airplay::Device> {
        let found = self
            .browser
            .as_ref()
            .map(Browser::devices)
            .unwrap_or_default();
        #[cfg(test)]
        let found = [found, self.fakes.clone()].concat();
        found
    }

    /// The receiver `device` is, as found now.
    fn receiver(&self, device: &Device) -> Result<::airplay::Device, Error> {
        self.found()
            .into_iter()
            .find(|found| found.id == device.id)
            .ok_or(Error::Gone)
    }
}

impl Target for AirPlayTarget {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// Receivers that show video; speakers are outputs of their own.
    fn devices(&self) -> Vec<Device> {
        self.found()
            .into_iter()
            .filter(::airplay::Device::shows_video)
            .filter_map(|receiver| {
                Some(Device {
                    kind: KIND,
                    shows: Shows::Video,
                    address: receiver.addresses.first()?.ip(),
                    id: receiver.id,
                    name: receiver.name,
                })
            })
            .collect()
    }

    fn connect(&self, device: &Device) -> Result<Box<dyn Session>, Error> {
        let receiver = self.receiver(device)?;
        let credentials = self.pairings.credentials(&receiver.id);
        let sender = self.pairings.sender();
        match VideoSession::connect(&receiver, credentials.as_ref(), &sender) {
            Ok((session, events)) => Ok(Box::new(AirPlaySession::new(
                session,
                events,
                profile(&receiver.model),
            ))),
            Err(::airplay::Error::NeedsPin) => Err(Error::NeedsPin),
            // It no longer knows us, as after it's reset: paired again,
            // which replaces what's kept. Kept till then: a receiver turning
            // its pairing down isn't to be taken at its word.
            Err(::airplay::Error::Forgotten(why)) => {
                log::info!("airplay: {} turned down its pairing: {why}", receiver.name);
                Err(Error::NeedsPin)
            }
            Err(err) => Err(failed_at(&receiver, &err)),
        }
    }

    fn pair(&self, device: &Device) -> Result<Box<dyn Pairing>, Error> {
        AirPlayPairing::start(self.receiver(device)?, Arc::clone(&self.pairings))
    }
}

/// What went wrong, as the block is told; why it can't be reached, or
/// can't play what it was told to, goes to the log.
fn failed(err: &::airplay::Error) -> Error {
    match err {
        err if err.is_gone() => {
            log::warn!("airplay: can't reach the receiver: {err}");
            Error::Gone
        }
        ::airplay::Error::Unplayable(why) => {
            log::warn!("airplay: the receiver can't play it: {why}");
            Error::Unplayable
        }
        err => Error::Failed(err.to_string()),
    }
}

/// `failed`, saying what to change on `receiver` if it turned this
/// computer away.
fn failed_at(receiver: &::airplay::Device, err: &::airplay::Error) -> Error {
    match err {
        ::airplay::Error::Refused => Error::Failed(turned_away(receiver)),
        err => failed(err),
    }
}

/// Why `receiver` turned this computer away, and what lets it in: a Mac
/// lets only its own user's devices play to it unless set otherwise, as
/// macOS sets it at first, and an Apple TV or HomePod may let only the
/// people of its home.
pub(crate) fn turned_away(receiver: &::airplay::Device) -> String {
    let model = receiver.model.as_str();
    let change = if model.starts_with("Mac") || model.starts_with("iMac") {
        "On it, set System Settings → General → AirDrop & Handoff → Allow AirPlay \
         for to Anyone on the Same Network or Everyone."
    } else if model.starts_with("AppleTV") || model.starts_with("AudioAccessory") {
        "In the Home app, set Home Settings → Speakers & TV → Allow Speaker & TV \
         Access to Anyone on the Same Network or Everyone."
    } else {
        "Its settings say which."
    };
    format!("it lets only some devices play to it. {change}")
}

/// How the stream server serves a receiver, by its model: an Apple TV 4K
/// (`AppleTV6,2` and later) HEVC up to 2160p with HDR10, HLG and Dolby
/// Vision; an Apple TV HD (`AppleTV5,3`) HEVC up to 1080p; an older Apple
/// TV H.264. Apple TVs take AC-3, E-AC-3 and ALAC too, up to 7.1, in HLS of
/// fragmented MP4, which HEVC and HDR need. Any other TV gets what nearly
/// every TV plays: H.264 and AAC in MPEG-TS. Always as HLS, never a file as
/// it is, so subtitles go in the playlist.
fn profile(model: &str) -> Profile {
    let plain = Profile {
        files: Vec::new(),
        hls_subtitles: true,
        ..Profile::default()
    };
    let Some(generation) = apple_tv(model) else {
        return plain;
    };
    let apple = Profile {
        audio: vec![
            AudioCodec::Aac,
            AudioCodec::Ac3,
            AudioCodec::Eac3,
            AudioCodec::Alac,
        ],
        max_channels: 8,
        ..plain
    };
    match generation {
        6.. => Profile {
            hls: Some(Segments::Fmp4),
            video: vec![VideoCodec::H264, VideoCodec::Hevc],
            max_size: Some(UHD),
            max_bit_depth: 10,
            hdr: vec![Hdr::Hdr10, Hdr::Hlg, Hdr::DolbyVision],
            ..apple
        },
        5 => Profile {
            hls: Some(Segments::Fmp4),
            video: vec![VideoCodec::H264, VideoCodec::Hevc],
            max_size: Some(FULL_HD),
            max_bit_depth: 10,
            ..apple
        },
        _ => apple,
    }
}

/// An Apple TV's generation, from its model: 6 in `AppleTV6,2`.
fn apple_tv(model: &str) -> Option<u32> {
    let (generation, _) = model.strip_prefix("AppleTV")?.split_once(',')?;
    generation.parse().ok()
}

/// Pairing with the PIN a receiver shows, kept once it's done: a TV cast
/// to, or a speaker played through.
pub(crate) struct AirPlayPairing {
    pairing: PinPairing,
    receiver: ::airplay::Device,
    pairings: Arc<Pairings>,
}

impl AirPlayPairing {
    /// Have `receiver` show a PIN, to finish pairing with, kept in
    /// `pairings`. Waits on the network.
    pub(crate) fn start(
        receiver: ::airplay::Device,
        pairings: Arc<Pairings>,
    ) -> Result<Box<dyn Pairing>, Error> {
        let pairing = PinPairing::start(&receiver).map_err(|err| failed_at(&receiver, &err))?;
        Ok(Box::new(Self {
            pairing,
            receiver,
            pairings,
        }))
    }
}

impl Pairing for AirPlayPairing {
    fn finish(self: Box<Self>, pin: &str) -> Result<(), Error> {
        let credentials = self
            .pairing
            .finish(pin)
            .map_err(|err| failed_at(&self.receiver, &err))?;
        self.pairings
            .keep(&self.receiver.id, &self.receiver.name, &credentials);
        Ok(())
    }
}

struct AirPlaySession {
    /// Until stopped.
    session: Option<VideoSession>,
    events: UnboundedReceiver<Event>,
    profile: Profile,
    /// The URL it was last given.
    url: Option<String>,
    /// How many times it was given the media again for other subtitles:
    /// each time at a URL of its own, so it reads the playlist anew.
    given: u32,
    /// What it last said it was doing.
    info: PlaybackInfo,
    /// Whether it was told to play.
    playing: bool,
    /// Whether it played to the end, after which it has nothing to play
    /// and is given the media again to play it.
    finished: bool,
    /// Where it was sent since, to play from.
    start_over: Option<f64>,
    /// The block's volume, which is the receiver's.
    volume: f32,
    muted: bool,
    /// It went away, or ended the session.
    gone: bool,
}

impl AirPlaySession {
    fn new(session: VideoSession, events: UnboundedReceiver<Event>, profile: Profile) -> Self {
        Self {
            session: Some(session),
            events,
            profile,
            url: None,
            given: 0,
            info: PlaybackInfo {
                state: PlaybackState::Loading,
                position: 0.0,
                duration: None,
            },
            playing: false,
            finished: false,
            start_over: None,
            volume: 1.0,
            muted: false,
            gone: false,
        }
    }

    fn session(&mut self) -> Result<&mut VideoSession, Error> {
        self.hear();
        if self.gone {
            return Err(Error::Gone);
        }
        self.session.as_mut().ok_or(Error::Gone)
    }

    /// Take in what it said since last asked.
    fn hear(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Gone => self.gone = true,
            }
        }
    }

    /// The URL to give it: the media's, marked with how many times it was
    /// given again.
    fn url(&self) -> Option<String> {
        let url = self.url.as_ref()?;
        Some(match self.given {
            0 => url.clone(),
            given => format!("{url}?{given}"),
        })
    }

    /// Give it the media to play from `start`, and play it with `play`.
    fn give(&mut self, play: bool, start: f64) -> Result<(), Error> {
        let url = self.url().ok_or(Error::Unplayable)?;
        self.session()?
            .play(&url, start, play)
            .map_err(|err| failed(&err))?;
        self.playing = play;
        self.finished = false;
        self.start_over = None;
        self.info = PlaybackInfo {
            state: PlaybackState::Loading,
            position: start,
            duration: self.info.duration,
        };
        // Each starts at full volume.
        if self.volume < 1.0 || self.muted {
            self.turn()?;
        }
        Ok(())
    }

    /// Set its volume to the block's.
    fn turn(&mut self) -> Result<(), Error> {
        let (volume, muted) = (self.volume, self.muted);
        self.session()?
            .set_volume(volume, muted)
            .map_err(|err| failed(&err))
    }

    /// Ask what it's doing now.
    fn ask(&mut self) -> Result<(), Error> {
        let info = self
            .session()?
            .playback_info()
            .map_err(|err| failed(&err))?;
        self.info = PlaybackInfo {
            duration: info.duration.or(self.info.duration),
            ..info
        };
        if info.state == PlaybackState::Ended {
            self.finished = true;
        }
        Ok(())
    }
}

impl Session for AirPlaySession {
    fn profile(&self) -> Profile {
        self.profile.clone()
    }

    fn load(&mut self, load: &Load) -> Result<(), Error> {
        self.url = Some(load.url.clone());
        self.given = 0;
        self.volume = load.volume;
        self.muted = load.muted;
        self.info.duration = load.duration;
        self.give(load.autoplay, load.start)
    }

    fn play(&mut self) -> Result<(), Error> {
        if self.finished {
            let start = self.start_over.unwrap_or(0.0);
            return self.give(true, start);
        }
        self.session()?.resume().map_err(|err| failed(&err))?;
        self.playing = true;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), Error> {
        self.playing = false;
        if self.finished {
            return Ok(());
        }
        self.session()?.pause().map_err(|err| failed(&err))
    }

    fn seek(&mut self, time: f64) -> Result<(), Error> {
        if self.finished {
            self.start_over = Some(time);
            return Ok(());
        }
        self.session()?.seek(time).map_err(|err| failed(&err))
    }

    fn set_volume(&mut self, volume: f32, muted: bool) -> Result<(), Error> {
        self.volume = volume;
        self.muted = muted;
        if self.url.is_none() || self.finished {
            return Ok(());
        }
        self.turn()
    }

    /// The playlist says what subtitles there are, and the stream server
    /// says them anew: it's given again, from where it is.
    fn set_subtitles(&mut self, _: Option<&Subtitles>) -> Result<(), Error> {
        if self.url.is_none() {
            return Ok(());
        }
        self.given += 1;
        if self.finished {
            return Ok(());
        }
        self.ask()?;
        if self.finished {
            return Ok(());
        }
        let (playing, position) = (self.playing, self.info.position);
        self.give(playing, position)
    }

    fn stop(&mut self) -> Result<(), Error> {
        if let Some(session) = self.session.take() {
            session.stop();
        }
        Ok(())
    }

    fn status(&mut self) -> Result<Status, Error> {
        if !self.finished {
            self.ask()?;
        }
        let (state, position) = match (self.finished, self.start_over) {
            (true, Some(time)) => (State::Paused, time),
            (true, None) => (
                State::Ended,
                self.info.duration.unwrap_or(self.info.position),
            ),
            (false, _) => (state(self.info.state), self.info.position),
        };
        Ok(Status {
            state,
            position,
            at: None,
            duration: self.info.duration,
        })
    }
}

/// What a block makes of what a receiver says it's doing.
fn state(state: PlaybackState) -> State {
    match state {
        PlaybackState::Loading => State::Buffering,
        PlaybackState::Playing => State::Playing,
        PlaybackState::Paused => State::Paused,
        PlaybackState::Ended => State::Ended,
    }
}

#[cfg(test)]
#[path = "tests/airplay.rs"]
mod tests;
