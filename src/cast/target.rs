//! What Plays asks of a casting protocol: finding devices, and playing on
//! one what the stream server serves. Each protocol's crate is adapted to
//! these traits.

use std::fmt;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use stream_server::{Profile, Segments, SubtitleSource};

use crate::settings::SubtitleStyle;

/// How often a device is asked where it is while a picture here follows
/// its sound.
pub const FOLLOW_POLL: Duration = Duration::from_millis(100);

/// What a device shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shows {
    /// Pictures and sound: a TV, or a stick in one.
    Video,
    /// Sound only: a speaker.
    Sound,
}

/// A device found on the network.
#[derive(Clone, PartialEq, Debug)]
pub struct Device {
    /// Its protocol's name, such as "Chromecast", which is its target's.
    pub kind: &'static str,
    /// Its id among its protocol's devices, which stays as its address
    /// changes.
    pub id: String,
    pub name: String,
    pub shows: Shows,
    /// Where it is, for the stream server to give it a URL on the network
    /// it is on.
    pub address: IpAddr,
}

impl Device {
    /// The same device, whatever its name or address now.
    pub fn is(&self, other: &Device) -> bool {
        self.kind == other.kind && self.id == other.id
    }
}

/// A protocol: the devices it finds, and how to reach them.
pub trait Target: Send + Sync {
    /// Its name, which its devices have as their `kind`.
    fn kind(&self) -> &'static str;

    /// The devices found so far. Finding them goes on in the background,
    /// so this only says what has been found.
    fn devices(&self) -> Vec<Device>;

    /// Connect to one of its devices. Waits on the network, so not for the
    /// UI thread.
    fn connect(&self, device: &Device) -> Result<Box<dyn Session>, Error>;

    /// Start pairing with a device whose `connect` said `Error::NeedsPin`,
    /// which has it show a PIN. Waits on the network too.
    fn pair(&self, device: &Device) -> Result<Box<dyn Pairing>, Error> {
        Err(Error::Failed(format!("{} doesn't pair", device.name)))
    }
}

/// Pairing with a device, which shows a PIN: finished with it, the target
/// keeps what pairing left, and connects with that from then on.
pub trait Pairing: Send {
    /// Waits on the network. A wrong PIN fails, and the device shows no
    /// PIN any more: pairing starts again for another.
    fn finish(self: Box<Self>, pin: &str) -> Result<(), Error>;
}

/// What a device says went wrong.
#[derive(Clone, PartialEq, Debug)]
pub enum Error {
    /// It's gone: turned off, or off the network.
    Gone,
    /// It plays something else now, as from its remote or another app.
    Replaced,
    /// It was stopped there, from its remote or another app.
    Stopped,
    /// It couldn't play the media.
    Unplayable,
    /// It didn't start playing the media in time, saying nothing of why.
    NotStarted,
    /// It needs to be paired with the PIN it shows first (`Target::pair`).
    NeedsPin,
    /// It refused, or said something unexpected.
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gone => f.write_str("it can't be reached"),
            Self::Replaced => f.write_str("it plays something else now"),
            Self::Stopped => f.write_str("it was stopped there"),
            Self::Unplayable => f.write_str("it can't play this"),
            Self::NotStarted => f.write_str("it didn't start playing"),
            Self::NeedsPin => f.write_str("it needs to be paired with the PIN it shows"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

/// Media for a device to play, as the stream server serves it.
#[derive(Clone, PartialEq, Debug)]
pub struct Load {
    pub url: String,
    pub mime: String,
    /// UPnP's `protocolInfo` for it.
    pub protocol_info: String,
    pub title: String,
    /// In seconds, when it has an end.
    pub duration: Option<f64>,
    /// A stream with no end, which can't seek.
    pub live: bool,
    /// For HLS this computer makes, what its segments are.
    pub segments: Option<Segments>,
    /// Where to start, in seconds.
    pub start: f64,
    pub autoplay: bool,
    /// The block's volume, 0 to 1.
    pub volume: f32,
    pub muted: bool,
    pub subtitles: Option<Subtitles>,
    /// The picture plays here, following the device's sound: it's asked
    /// where it is every `FOLLOW_POLL`.
    pub picture_here: bool,
    /// A stream of the sound alone this computer makes, which starts where
    /// `start=` in its URL says. Speakers can't seek in it (they say it's
    /// an illegal seek target) and say how long it is as how much they have
    /// played, so a device is given it from where it's to start, again to
    /// go elsewhere, and says where it is from there.
    pub starts_at_url: bool,
}

/// Subtitles served beside the media.
#[derive(Clone, PartialEq, Debug)]
pub struct Subtitles {
    /// Where they come from: the same at the same URLs, else others.
    pub source: SubtitleSource,
    pub webvtt_url: String,
    pub srt_url: String,
    /// ISO 639-2, as the file has it.
    pub language: Option<String>,
    /// What the block's track menu calls them.
    pub name: String,
    /// How Settings say subtitles look, for devices that draw them as told.
    pub style: SubtitleStyle,
}

/// What a device is doing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Status {
    pub state: State,
    /// Where it is in the media, in seconds.
    pub position: f64,
    /// When it was there, where it says; otherwise it was when asked.
    pub at: Option<Instant>,
    /// How long the media is, when the device says.
    pub duration: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Getting the media, or waiting for more of it.
    Buffering,
    Playing,
    Paused,
    /// Played to the end.
    Ended,
}

/// Control of one device, from one thread at a time.
pub trait Session: Send {
    /// What the device plays, for the stream server to serve it that way.
    fn profile(&self) -> Profile;

    fn load(&mut self, load: &Load) -> Result<(), Error>;

    fn play(&mut self) -> Result<(), Error>;

    fn pause(&mut self) -> Result<(), Error>;

    fn seek(&mut self, time: f64) -> Result<(), Error>;

    /// The block's volume, 0 to 1. Where the protocol has a volume for the
    /// media apart from the device's own, it is that one.
    fn set_volume(&mut self, volume: f32, muted: bool) -> Result<(), Error>;

    /// Show these subtitles, or none.
    fn set_subtitles(&mut self, subtitles: Option<&Subtitles>) -> Result<(), Error>;

    /// Stop playing. The session ends here.
    fn stop(&mut self) -> Result<(), Error>;

    /// What it is doing now; `Error::Gone` once it's gone.
    fn status(&mut self) -> Result<Status, Error>;
}
