//! Having a TV play a URL: AirPlay 2's session, then `/play` with the URL
//! and where to start, and `/rate`, `/scrub` and `/stop` to drive it. What
//! it's doing comes from `/playback-info`, asked for as often as wanted.

use futures_channel::mpsc::UnboundedReceiver;
use plist::{Dictionary, Value};

use crate::audio::volume_db;
use crate::connection::{Connection, Request};
use crate::device::Device;
use crate::error::{Error, Result};
use crate::pairing::Credentials;
use crate::session::{Control, Event, Sender};

/// Within this of the end, a stopped item has played to it.
const END: f64 = 0.5;

/// What a TV is doing with what it was told to play.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlaybackInfo {
    pub state: PlaybackState,
    /// Seconds in.
    pub position: f64,
    /// Seconds, once it knows.
    pub duration: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaybackState {
    /// Getting it, or waiting for more of it.
    Loading,
    Playing,
    Paused,
    /// Played to the end, or stopped.
    Ended,
}

/// Pairing with the PIN a TV shows: started, which has it show the PIN,
/// then finished with the PIN.
pub struct PinPairing {
    connection: Connection,
}

impl PinPairing {
    pub fn start(device: &Device) -> Result<Self> {
        let mut connection = Connection::open(&device.addresses)?;
        connection.start_pin()?;
        Ok(Self { connection })
    }

    pub fn finish(mut self, pin: &str) -> Result<Credentials> {
        self.connection.pair_pin(pin)
    }
}

pub struct VideoSession {
    control: Control,
    /// Whether anything was seen playing, to tell an end from a start.
    started: bool,
    /// The item's id, which `/play` names.
    uuid: String,
    /// The sender's id, which `/play` says too.
    sender_id: String,
}

fn int(n: i64) -> Value {
    Value::Integer(n.into())
}

impl VideoSession {
    /// Connect to `device`, verified with what pairing left.
    pub fn connect(
        device: &Device,
        credentials: Option<&Credentials>,
        sender: &Sender,
    ) -> Result<(Self, UnboundedReceiver<Event>)> {
        if !device.shows_video() {
            return Err(Error::protocol("it doesn't show video"));
        }
        let (mut control, events) = Control::connect(device, credentials)?;
        control.setup(sender)?;
        let uri = control.uri();
        control.exchange(Request::rtsp("RECORD", uri))?;
        let uuid = uuid();
        Ok((
            Self {
                control,
                started: false,
                uuid,
                sender_id: sender.id.clone(),
            },
            events,
        ))
    }

    fn post(&self, path: String) -> Result<()> {
        self.control.exchange(Request::http("POST", path))?;
        Ok(())
    }

    fn set_property(&self, name: &str, value: Value) -> Result<()> {
        let mut body = Dictionary::new();
        body.insert("value".into(), value);
        self.control.exchange(
            Request::http("PUT", format!("/setProperty?{name}")).plist(Value::Dictionary(body)),
        )?;
        Ok(())
    }

    /// Play `url` from `start` seconds in; paused there unless `autoplay`.
    pub fn play(&mut self, url: &str, start: f64, autoplay: bool) -> Result<()> {
        let mut body = Dictionary::new();
        let mut put = |key: &str, value: Value| {
            body.insert(key.to_owned(), value);
        };
        put("Content-Location", url.into());
        put("Start-Position-Seconds", start.into());
        put("uuid", self.uuid.clone().into());
        put("streamType", int(1));
        put("mediaType", "file".into());
        put("mightSupportStorePastisKeyRequests", true.into());
        put("playbackRestrictions", int(0));
        put("referenceRestrictions", int(3));
        put("volume", 1.0.into());
        put("rate", if autoplay { 1.0 } else { 0.0 }.into());
        put("SenderMACAddress", self.sender_id.clone().into());
        put("model", "iPhone14,3".into());
        put("osBuildVersion", "20G1116".into());
        put("clientBundleID", "com.apple.TVRemote".into());
        put("clientProcName", "AirPlay".into());
        put("secureConnectionMs", int(22));
        put("infoMs", int(122));
        put("connectMs", int(18));
        put("authMs", int(0));
        put("bonjourMs", int(0));
        put("postAuthMs", int(0));
        self.control.exchange(
            Request::http("POST", "/play")
                .header("X-Apple-ProtocolVersion", 1)
                .header("X-Apple-Stream-ID", 1)
                .header("X-Apple-Session-ID", self.uuid.to_lowercase())
                .plist(Value::Dictionary(body)),
        )?;
        self.started = false;
        self.set_property("isInterestedInDateRange", true.into())?;
        self.set_property("actionAtItemEnd", int(0))?;
        self.set_rate(if autoplay { 1.0 } else { 0.0 })?;
        // No end times: it plays to the end.
        for end in ["forwardEndTime", "reverseEndTime"] {
            let mut time = Dictionary::new();
            for key in ["flags", "value", "epoch", "timescale"] {
                time.insert(key.into(), int(0));
            }
            self.set_property(end, Value::Dictionary(time))?;
        }
        Ok(())
    }

    fn set_rate(&self, rate: f64) -> Result<()> {
        self.post(format!("/rate?value={rate:.6}"))
    }

    pub fn resume(&mut self) -> Result<()> {
        self.set_rate(1.0)
    }

    pub fn pause(&mut self) -> Result<()> {
        self.set_rate(0.0)
    }

    /// Go to `position` seconds in.
    pub fn seek(&mut self, position: f64) -> Result<()> {
        self.post(format!("/scrub?position={position:.6}"))
    }

    pub fn set_volume(&mut self, volume: f32, muted: bool) -> Result<()> {
        let uri = self.control.uri();
        self.control.exchange(
            Request::rtsp("SET_PARAMETER", uri)
                .parameters(format!("volume: {:.6}\r\n", volume_db(volume, muted))),
        )?;
        Ok(())
    }

    /// What it's doing now.
    pub fn playback_info(&mut self) -> Result<PlaybackInfo> {
        let info = self
            .control
            .exchange(Request::http("GET", "/playback-info"))?
            .dictionary()
            .unwrap_or_default();
        let info = read_playback_info(&info, self.started);
        if matches!(info.state, PlaybackState::Playing | PlaybackState::Paused) {
            self.started = true;
        }
        Ok(info)
    }

    /// Stop playing, and end the session, asking nothing of a receiver
    /// that's gone or ended it.
    pub fn stop(self) {
        if !self.control.gone()
            && let Err(err) = self.post("/stop".into())
        {
            log::debug!("airplay: stop: {err}");
        }
        self.control.stop();
    }
}

fn number(info: &Dictionary, key: &str) -> Option<f64> {
    let value = info.get(key)?;
    value
        .as_real()
        .or_else(|| value.as_signed_integer().map(|n| n as f64))
}

fn flag(info: &Dictionary, key: &str) -> bool {
    info.get(key).and_then(Value::as_boolean).unwrap_or(false)
}

/// What `/playback-info` says; `started` is whether it was seen playing,
/// as it says nothing at all both before and after.
fn read_playback_info(info: &Dictionary, started: bool) -> PlaybackInfo {
    let position = number(info, "position").unwrap_or(0.0);
    let duration = number(info, "duration").filter(|duration| *duration > 0.0);
    let rate = number(info, "rate").unwrap_or(0.0);
    let state = match duration {
        None if started => PlaybackState::Ended,
        None => PlaybackState::Loading,
        Some(_) if !flag(info, "readyToPlay") => PlaybackState::Loading,
        Some(_) if rate > 0.0 && flag(info, "playbackBufferEmpty") => PlaybackState::Loading,
        Some(_) if rate > 0.0 => PlaybackState::Playing,
        Some(duration) if position >= duration - END => PlaybackState::Ended,
        Some(_) => PlaybackState::Paused,
    };
    PlaybackInfo {
        state,
        position,
        duration,
    }
}

fn uuid() -> String {
    let bytes: [u8; 16] = rand::random();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[cfg(test)]
#[path = "tests/video.rs"]
mod tests;
