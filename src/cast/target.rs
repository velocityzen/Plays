//! What Plays asks of a casting protocol: finding devices, and playing on
//! one what the stream server serves. Each protocol's crate is adapted to
//! these traits.

use std::fmt;
use std::net::IpAddr;

use stream_server::Profile;

use crate::settings::SubtitleStyle;

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
}

/// What a device says went wrong.
#[derive(Clone, PartialEq, Debug)]
pub enum Error {
    /// It's gone: turned off, or off the network.
    Gone,
    /// It plays something else now, as from its remote or another app.
    Replaced,
    /// It refused, or said something unexpected.
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gone => f.write_str("it can't be reached"),
            Self::Replaced => f.write_str("it plays something else now"),
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
    /// Where to start, in seconds.
    pub start: f64,
    pub autoplay: bool,
    /// The tile's volume, 0 to 1.
    pub volume: f32,
    pub muted: bool,
    pub subtitles: Option<Subtitles>,
}

/// Subtitles served beside the media.
#[derive(Clone, PartialEq, Debug)]
pub struct Subtitles {
    pub webvtt_url: String,
    pub srt_url: String,
    /// ISO 639-2, as the file has it.
    pub language: Option<String>,
    /// What the tile's track menu calls them.
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

    /// The tile's volume, 0 to 1. Where the protocol has a volume for the
    /// media apart from the device's own, it is that one.
    fn set_volume(&mut self, volume: f32, muted: bool) -> Result<(), Error>;

    /// Show these subtitles, or none.
    fn set_subtitles(&mut self, subtitles: Option<&Subtitles>) -> Result<(), Error>;

    /// Stop playing. The session ends here.
    fn stop(&mut self) -> Result<(), Error>;

    /// What it is doing now; `Error::Gone` once it's gone.
    fn status(&mut self) -> Result<Status, Error>;
}
