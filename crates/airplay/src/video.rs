//! Having a TV play a URL: AirPlay 2's session, then a remote-control
//! stream on it, where the URL is queued and commands drive it; what it's
//! doing it answers on the event connection, asked as often as wanted.
//! tvOS 18 and later want it so: they take `/play` and play nothing. A TV
//! that refuses the stream is told with `/play`, driven with `/rate`,
//! `/scrub` and `/stop`, and asked with `/playback-info`.

use futures_channel::mpsc::UnboundedReceiver;
use plist::{Dictionary, Value};

use crate::audio::volume_db;
use crate::connection::{Connection, Request};
use crate::device::Device;
use crate::error::{Error, Result};
use crate::pairing::Credentials;
use crate::session::{Control, Event, Sender, command_body, unsigned};

/// Within this of the end, a stopped item has played to it.
const END: f64 = 0.5;
/// The remote-control stream's type, and the kind of client it's for.
const REMOTE_CONTROL: u64 = 130;
const REMOTE_CONTROL_CLIENT: &str = "A6B27562-B43A-4F2D-B75F-82391E250194";

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

/// How a TV is told what to play.
enum Way {
    /// Queued on the remote-control stream with this id.
    Queue { stream: String },
    /// With `/play`.
    Play,
}

pub struct VideoSession {
    control: Control,
    way: Way,
    /// Whether anything was seen playing, to tell an end from a start, as
    /// `/playback-info` says nothing both before and after.
    started: bool,
    /// The item's id, new for each queued; the one `/play` names.
    uuid: String,
    /// Whether an item was queued, which the next replaces.
    queued: bool,
    /// The sender's id, which `/play` says too.
    sender_id: String,
    /// The last command that asked, by number.
    asked: u64,
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
        let way = remote_control(&control, sender)?;
        Ok((
            Self {
                control,
                way,
                started: false,
                uuid: uuid(),
                queued: false,
                sender_id: sender.id.clone(),
                asked: 0,
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

    /// A command on the remote-control stream: `kind`, with `fields`.
    fn command<const N: usize>(&self, kind: &str, fields: [(&str, Value); N]) -> Result<()> {
        let Way::Queue { stream } = &self.way else {
            return Err(Error::protocol("no remote-control stream"));
        };
        let mut command = Dictionary::new();
        command.insert("type".into(), kind.into());
        for (key, value) in fields {
            command.insert(key.to_owned(), value);
        }
        self.control.exchange(
            Request::http("POST", "/command")
                .header("X-Apple-ProtocolVersion", 1)
                .header("X-Apple-Session-ID", &self.control.session_uuid)
                .header("X-Apple-StreamID", stream)
                .plist(command_body(command)),
        )?;
        Ok(())
    }

    /// Play `url` from `start` seconds in; paused there unless `autoplay`.
    pub fn play(&mut self, url: &str, start: f64, autoplay: bool) -> Result<()> {
        if matches!(self.way, Way::Play) {
            return self.play_url(url, start, autoplay);
        }
        let before = std::mem::replace(&mut self.uuid, uuid());
        let mut item = Dictionary::new();
        item.insert("uuid".into(), self.uuid.clone().into());
        item.insert("mediaType".into(), "file".into());
        item.insert("Content-Location".into(), url.into());
        if start > 0.0 {
            item.insert("Start-Position".into(), time(start));
        }
        self.command("insertPlayQueueItem", [("item", Value::Dictionary(item))])?;
        // The one before plays on, or stays ended, till it goes.
        if std::mem::replace(&mut self.queued, true) {
            self.command("removePlayQueueItem", [("item", item_named(&before))])?;
        }
        // Paused at its end, rather than gone.
        self.command(
            "setProperty",
            [("property", "actionAtItemEnd".into()), ("value", int(1))],
        )?;
        self.set_rate(if autoplay { 1.0 } else { 0.0 })
    }

    fn play_url(&mut self, url: &str, start: f64, autoplay: bool) -> Result<()> {
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
        match self.way {
            Way::Queue { .. } => self.command("setRate", [("rate", rate.into())]),
            Way::Play => self.post(format!("/rate?value={rate:.6}")),
        }
    }

    pub fn resume(&mut self) -> Result<()> {
        self.set_rate(1.0)
    }

    pub fn pause(&mut self) -> Result<()> {
        self.set_rate(0.0)
    }

    /// Go to `position` seconds in.
    pub fn seek(&mut self, position: f64) -> Result<()> {
        match self.way {
            // Its item named, or it goes nowhere.
            Way::Queue { .. } => self.command(
                "seek",
                [("time", time(position)), ("item", item_named(&self.uuid))],
            ),
            Way::Play => self.post(format!("/scrub?position={position:.6}")),
        }
    }

    pub fn set_volume(&mut self, volume: f32, muted: bool) -> Result<()> {
        let uri = self.control.uri();
        self.control.exchange(
            Request::rtsp("SET_PARAMETER", uri)
                .parameters(format!("volume: {:.6}\r\n", volume_db(volume, muted))),
        )?;
        Ok(())
    }

    /// What it's doing now; why not, if it couldn't play what it was told
    /// to.
    pub fn playback_info(&mut self) -> Result<PlaybackInfo> {
        if matches!(self.way, Way::Queue { .. }) {
            let info = self.ask_queue()?;
            if let Some(why) = failure(&info) {
                return Err(Error::Unplayable(why));
            }
            return Ok(read_queue_info(&info, &self.uuid));
        }
        let info = self
            .control
            .exchange(Request::http("GET", "/playback-info"))?
            .dictionary()
            .unwrap_or_default();
        if let Some(why) = failure(&info) {
            return Err(Error::Unplayable(why));
        }
        let info = read_playback_info(&info, self.started);
        if matches!(info.state, PlaybackState::Playing | PlaybackState::Paused) {
            self.started = true;
        }
        Ok(info)
    }

    /// What the queue says it's doing.
    fn ask_queue(&mut self) -> Result<Dictionary> {
        self.asked += 1;
        let id = self.asked;
        self.command(
            "playbackInfo",
            [
                ("kind", "request".into()),
                ("messageID", Value::Integer(id.into())),
            ],
        )?;
        let answer = self.control.answer(id)?;
        Ok(answer
            .get("info")
            .and_then(Value::as_dictionary)
            .cloned()
            .unwrap_or_default())
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

/// A remote-control stream to queue on, if the receiver takes one.
fn remote_control(control: &Control, sender: &Sender) -> Result<Way> {
    let mut stream = Dictionary::new();
    stream.insert("type".into(), Value::Integer(REMOTE_CONTROL.into()));
    stream.insert("controlType".into(), int(1));
    stream.insert("channelID".into(), format!("{}-RCS-1", sender.id).into());
    stream.insert("clientUUID".into(), uuid().into());
    stream.insert("clientTypeUUID".into(), REMOTE_CONTROL_CLIENT.into());
    let mut body = Dictionary::new();
    body.insert(
        "streams".into(),
        Value::Array(vec![Value::Dictionary(stream)]),
    );
    let request = Request::rtsp("SETUP", control.uri()).plist(Value::Dictionary(body));
    let answer = match control.exchange(request) {
        Ok(answer) => answer.dictionary().unwrap_or_default(),
        Err(Error::Status { status, .. }) => {
            log::debug!("airplay: no remote-control stream ({status}); told with /play");
            return Ok(Way::Play);
        }
        Err(err) => return Err(err),
    };
    let id = answer
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| streams.first())
        .and_then(Value::as_dictionary)
        .and_then(|stream| stream.get("streamID"));
    Ok(match unsigned(id) {
        Some(id) => Way::Queue {
            stream: id.to_string(),
        },
        None => Way::Play,
    })
}

/// A queued item, by its id.
fn item_named(uuid: &str) -> Value {
    let mut item = Dictionary::new();
    item.insert("uuid".into(), uuid.into());
    Value::Dictionary(item)
}

/// `seconds`, as a time in commands: to the millisecond.
pub(crate) fn time(seconds: f64) -> Value {
    let mut time = Dictionary::new();
    time.insert(
        "value".into(),
        Value::Integer(((seconds * 1000.0).round() as i64).into()),
    );
    time.insert("timescale".into(), int(1000));
    time.insert("flags".into(), int(1));
    time.insert("epoch".into(), int(0));
    Value::Dictionary(time)
}

/// The seconds a time in commands is, if it's a time: valid, and neither
/// infinite nor indefinite.
pub(crate) fn seconds(time: Option<&Value>) -> Option<f64> {
    let time = time?.as_dictionary()?;
    let field = |key: &str| time.get(key).and_then(Value::as_signed_integer);
    let flags = field("flags")?;
    let timescale = field("timescale").filter(|timescale| *timescale > 0)?;
    if flags & 1 == 0 || flags & 0x1C != 0 {
        return None;
    }
    Some(field("value")? as f64 / timescale as f64)
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

/// Why `/playback-info` says it couldn't play what it was told to, if it
/// does: its player's error.
fn failure(info: &Dictionary) -> Option<String> {
    let error = info.get("error")?;
    let field = |key: &str| error.as_dictionary().and_then(|error| error.get(key));
    let domain = field("domain")
        .and_then(Value::as_string)
        .unwrap_or("an unknown error");
    Some(match field("code").and_then(Value::as_signed_integer) {
        Some(code) => format!("{domain} {code}"),
        None => domain.to_owned(),
    })
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

/// What the queue says of `item`, the one it was last given: its own
/// state, while that's ready. Till it has it, it may speak of the one
/// before, which is loading this one.
fn read_queue_info(info: &Dictionary, item: &str) -> PlaybackInfo {
    let of = info
        .get("item")
        .and_then(Value::as_dictionary)
        .and_then(|of| of.get("uuid"))
        .and_then(Value::as_string);
    if of.is_some_and(|of| of != item) {
        return PlaybackInfo {
            state: PlaybackState::Loading,
            position: 0.0,
            duration: None,
        };
    }
    let position = seconds(info.get("position")).unwrap_or(0.0);
    let duration = seconds(info.get("duration")).filter(|duration| *duration > 0.0);
    let state = match info.get("playbackState").and_then(Value::as_string) {
        Some("stopped") => PlaybackState::Ended,
        // Paused, as it says when what it was given doesn't come.
        _ if !flag(info, "readyToPlay") => PlaybackState::Loading,
        Some("playing") if flag(info, "playbackBufferEmpty") => PlaybackState::Loading,
        Some("playing") => PlaybackState::Playing,
        Some("paused") if duration.is_some_and(|duration| position >= duration - END) => {
            PlaybackState::Ended
        }
        Some("paused") => PlaybackState::Paused,
        _ => PlaybackState::Loading,
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
