//! A cast: the Default Media Receiver launched on a device, given a URL
//! and driven while it plays it. Requests wait for their answers, matched
//! by request id; a thread of the session's reads everything the device
//! says, keeps the heartbeat, and tells what happens over a channel.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use serde_json::{Value, json};

use crate::channel::{Channel, Incoming};
use crate::device::Device;
use crate::error::{Error, Result};
use crate::media::{IdleReason, Media, MediaStatus, PlayerState, TextTrackStyle, Volume};
use crate::proto::{CONNECTION, CastMessage, HEARTBEAT, MEDIA, PLATFORM, RECEIVER, kind};

/// The Default Media Receiver, Google's app that plays a URL.
pub const DEFAULT_MEDIA_RECEIVER: &str = "CC1AD845";
const SENDER: &str = "sender-0";

/// How often to ping the device, and how long it may say nothing before
/// it's taken to be gone; how often the reading thread looks up.
#[cfg(not(test))]
const HEARTBEAT_EVERY: Duration = Duration::from_secs(5);
#[cfg(not(test))]
const SILENCE: Duration = Duration::from_secs(15);
#[cfg(not(test))]
const TICK: Duration = Duration::from_millis(500);
#[cfg(test)]
const HEARTBEAT_EVERY: Duration = Duration::from_millis(100);
#[cfg(test)]
const SILENCE: Duration = Duration::from_millis(500);
#[cfg(test)]
const TICK: Duration = Duration::from_millis(20);

/// How long a device has to answer: a request, launching the app, and
/// loading what's cast, which it fetches first.
const ANSWER: Duration = Duration::from_secs(10);
const LAUNCH: Duration = Duration::from_secs(20);
const LOAD: Duration = Duration::from_secs(30);

/// How a LOAD plays.
#[derive(Clone, PartialEq, Debug)]
pub struct Load {
    /// Seconds in to start at.
    pub start: f64,
    /// Play at once, or wait paused.
    pub autoplay: bool,
    /// The text tracks to show.
    pub active_tracks: Vec<u32>,
    pub style: Option<TextTrackStyle>,
}

impl Default for Load {
    fn default() -> Self {
        Self {
            start: 0.0,
            autoplay: true,
            active_tracks: Vec::new(),
            style: None,
        }
    }
}

/// What a session says as it goes.
#[derive(Clone, PartialEq, Debug)]
pub enum Event {
    /// What's playing, whenever it changes.
    Media(MediaStatus),
    /// The device's volume changed.
    Volume(Volume),
    /// The cast is over; nothing more comes.
    Ended(Ended),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    /// The device went away, or stopped answering.
    Gone,
    /// The app closed: on the device, or another sender launched another.
    Stopped,
    /// Another sender played something else in its place.
    Replaced,
}

#[derive(Default)]
struct State {
    /// The launched app's session, and where its messages go.
    app_session: Option<String>,
    transport: Option<String>,
    /// What's loaded, as the device numbers it.
    media_session: Option<u64>,
    /// The request id of a LOAD under way.
    loading: Option<u32>,
    volume: Option<Volume>,
}

struct Inner {
    channel: Channel,
    next_request: AtomicU32,
    /// Requests waiting for their answers.
    waiting: Mutex<HashMap<u32, mpsc::Sender<Value>>>,
    state: Mutex<State>,
    ended: AtomicBool,
    /// Whether we are ending it, so it ending isn't news.
    stopping: AtomicBool,
    events: UnboundedSender<Event>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn send(&self, namespace: &str, destination: &str, payload: &Value) -> io::Result<()> {
        self.channel
            .send(&CastMessage::json(SENDER, destination, namespace, payload))
    }

    fn next_id(&self) -> u32 {
        self.next_request.fetch_add(1, Ordering::Relaxed)
    }

    /// Send `payload` as request `id` and wait up to `timeout` for the
    /// answer, which isn't to be a refusal.
    fn request_as(
        &self,
        id: u32,
        namespace: &str,
        destination: &str,
        mut payload: Value,
        timeout: Duration,
    ) -> Result<Value> {
        let what = kind(&payload).to_owned();
        payload["requestId"] = id.into();
        let (answer, answered) = mpsc::channel();
        lock(&self.waiting).insert(id, answer);
        if self.ended.load(Ordering::Relaxed) {
            lock(&self.waiting).remove(&id);
            return Err(Error::Ended);
        }
        if let Err(err) = self.send(namespace, destination, &payload) {
            lock(&self.waiting).remove(&id);
            return Err(err.into());
        }
        let reply = match answered.recv_timeout(timeout) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Timeout) => {
                lock(&self.waiting).remove(&id);
                return Err(Error::Timeout(what));
            }
            Err(RecvTimeoutError::Disconnected) => return Err(Error::Ended),
        };
        match kind(&reply) {
            "LOAD_FAILED"
            | "LOAD_CANCELLED"
            | "INVALID_REQUEST"
            | "INVALID_PLAYER_STATE"
            | "LAUNCH_ERROR"
            | "ERROR" => Err(Error::Refused {
                request: what,
                reason: reply["reason"]
                    .as_str()
                    .or(reply["detailedErrorCode"].as_str())
                    .unwrap_or(kind(&reply))
                    .to_owned(),
            }),
            _ => Ok(reply),
        }
    }

    fn request(
        &self,
        namespace: &str,
        destination: &str,
        payload: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.request_as(self.next_id(), namespace, destination, payload, timeout)
    }

    /// Take what the device said; says if the cast is over.
    fn handle(&self, message: &CastMessage) -> Option<Ended> {
        let payload = message.payload();
        let source = message.source_id.as_str();
        let ended = match (message.namespace.as_str(), kind(&payload)) {
            (HEARTBEAT, "PING") => {
                let _ = self.send(HEARTBEAT, source, &json!({"type": "PONG"}));
                None
            }
            (CONNECTION, "CLOSE") if source == PLATFORM => Some(Ended::Gone),
            (CONNECTION, "CLOSE") => {
                (self.state().transport.as_deref() == Some(source)).then_some(Ended::Stopped)
            }
            (RECEIVER, "RECEIVER_STATUS") => self.receiver_status(&payload),
            (MEDIA, "MEDIA_STATUS") => self.media_status(&payload),
            _ => None,
        };
        // The answer to a request goes to it even when it ends the cast,
        // as the answer to STOP does.
        let id = payload["requestId"]
            .as_u64()
            .and_then(|id| u32::try_from(id).ok());
        if let Some(answer) = id.and_then(|id| lock(&self.waiting).remove(&id)) {
            let _ = answer.send(payload);
        }
        ended
    }

    fn receiver_status(&self, payload: &Value) -> Option<Ended> {
        let status = &payload["status"];
        let mut state = self.state();
        if let Some(volume) = Volume::read(&status["volume"])
            && state.volume != Some(volume)
        {
            state.volume = Some(volume);
            let _ = self.events.unbounded_send(Event::Volume(volume));
        }
        let ours = state.app_session.as_deref()?;
        let running = status["applications"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|app| app["sessionId"].as_str() == Some(ours));
        (!running).then_some(Ended::Stopped)
    }

    fn media_status(&self, payload: &Value) -> Option<Ended> {
        let request = payload["requestId"].as_u64();
        let mut state = self.state();
        for status in payload["status"].as_array().into_iter().flatten() {
            let Some(status) = MediaStatus::read(status) else {
                continue;
            };
            if state.loading.is_some() {
                // Ours once the LOAD's answer says so; until then, whatever
                // it says is about what was before.
                if state.loading.map(u64::from) == request {
                    state.media_session = Some(status.media_session);
                }
            } else if let Some(ours) = state.media_session {
                let replaced = status.media_session != ours
                    || status.state == PlayerState::Idle(Some(IdleReason::Interrupted));
                if replaced {
                    return Some(Ended::Replaced);
                }
            }
            let _ = self.events.unbounded_send(Event::Media(status));
        }
        None
    }

    /// The cast is over: the waiting requests too, and the connection.
    fn end(&self, why: Option<Ended>) {
        self.ended.store(true, Ordering::Relaxed);
        lock(&self.waiting).clear();
        if let Some(why) = why
            && !self.stopping.load(Ordering::Relaxed)
        {
            log::info!("chromecast: the cast ended: {why:?}");
            let _ = self.events.unbounded_send(Event::Ended(why));
        }
        self.channel.close();
    }
}

/// Read what the device says until the cast is over, keeping the
/// heartbeat.
fn read(inner: &Inner, mut incoming: Incoming) {
    let _ = incoming.set_timeout(Some(TICK));
    let mut heard = Instant::now();
    let mut pinged = Instant::now();
    let ended = loop {
        match incoming.next() {
            Ok(Some(message)) => {
                heard = Instant::now();
                if let Some(ended) = inner.handle(&message) {
                    break Some(ended);
                }
            }
            Ok(None) => break Some(Ended::Gone),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(err) => {
                log::debug!("chromecast: {err}");
                break Some(Ended::Gone);
            }
        }
        if heard.elapsed() > SILENCE {
            log::info!("chromecast: the device stopped answering");
            break Some(Ended::Gone);
        }
        if pinged.elapsed() >= HEARTBEAT_EVERY {
            pinged = Instant::now();
            let _ = inner.send(HEARTBEAT, PLATFORM, &json!({"type": "PING"}));
        }
    };
    inner.end(ended);
}

/// A cast to a device.
pub struct Session {
    inner: Arc<Inner>,
    reader: Option<JoinHandle<()>>,
}

impl Session {
    /// Connect to `device` and launch the Default Media Receiver on it.
    pub fn connect(device: &Device) -> Result<(Self, UnboundedReceiver<Event>)> {
        let (channel, incoming) = Channel::connect(&device.addresses)?;
        let (events, receiver) = unbounded();
        let inner = Arc::new(Inner {
            channel,
            next_request: AtomicU32::new(1),
            waiting: Mutex::default(),
            state: Mutex::default(),
            ended: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            events,
        });
        inner.send(CONNECTION, PLATFORM, &json!({"type": "CONNECT"}))?;
        let reading = Arc::clone(&inner);
        let reader = thread::Builder::new()
            .name("chromecast session".into())
            .spawn(move || read(&reading, incoming))?;
        let session = Self {
            inner,
            reader: Some(reader),
        };
        session.launch()?;
        Ok((session, receiver))
    }

    /// Launch the app, or join it if it's running, and connect to it.
    fn launch(&self) -> Result<()> {
        let launch = json!({"type": "LAUNCH", "appId": DEFAULT_MEDIA_RECEIVER});
        let mut status = self.inner.request(RECEIVER, PLATFORM, launch, LAUNCH)?;
        let deadline = Instant::now() + LAUNCH;
        // It may answer before the app is up; it's there once it says so.
        let app = loop {
            let app = status["status"]["applications"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|app| {
                    app["appId"] == DEFAULT_MEDIA_RECEIVER && app["transportId"].is_string()
                })
                .cloned();
            if let Some(app) = app {
                break app;
            }
            if Instant::now() > deadline {
                return Err(Error::Timeout("LAUNCH".into()));
            }
            thread::sleep(Duration::from_millis(250));
            let get = json!({"type": "GET_STATUS"});
            status = self.inner.request(RECEIVER, PLATFORM, get, ANSWER)?;
        };
        let transport = app["transportId"].as_str().unwrap_or_default().to_owned();
        {
            let mut state = self.inner.state();
            state.app_session = app["sessionId"].as_str().map(str::to_owned);
            state.transport = Some(transport.clone());
            state.volume = Volume::read(&status["status"]["volume"]);
        }
        self.inner
            .send(CONNECTION, &transport, &json!({"type": "CONNECT"}))?;
        Ok(())
    }

    fn transport(&self) -> Result<String> {
        self.inner.state().transport.clone().ok_or(Error::Ended)
    }

    /// What's loaded, as the device numbers it.
    fn media_session(&self) -> Result<u64> {
        self.inner
            .state()
            .media_session
            .ok_or_else(|| Error::protocol("nothing loaded"))
    }

    /// The first status in a MEDIA_STATUS answer.
    fn media_answer(answer: &Value) -> Result<MediaStatus> {
        answer["status"]
            .as_array()
            .and_then(|status| status.first())
            .and_then(MediaStatus::read)
            .ok_or_else(|| Error::protocol("a media answer without the media"))
    }

    /// Have it play `media`, as `load` says.
    pub fn load(&self, media: &Media, load: &Load) -> Result<MediaStatus> {
        let transport = self.transport()?;
        let payload = json!({
            "type": "LOAD",
            "media": media.json(load.style.as_ref()),
            "autoplay": load.autoplay,
            "currentTime": load.start,
            "activeTrackIds": load.active_tracks,
        });
        let id = self.inner.next_id();
        self.inner.state().loading = Some(id);
        let answer = self.inner.request_as(id, MEDIA, &transport, payload, LOAD);
        self.inner.state().loading = None;
        Self::media_answer(&answer?)
    }

    /// A media command for what's loaded.
    fn command(&self, mut payload: Value) -> Result<MediaStatus> {
        let transport = self.transport()?;
        payload["mediaSessionId"] = self.media_session()?.into();
        let answer = self.inner.request(MEDIA, &transport, payload, ANSWER)?;
        Self::media_answer(&answer)
    }

    pub fn play(&self) -> Result<MediaStatus> {
        self.command(json!({"type": "PLAY"}))
    }

    pub fn pause(&self) -> Result<MediaStatus> {
        self.command(json!({"type": "PAUSE"}))
    }

    /// Go to `position` seconds in, playing or paused as it was.
    pub fn seek(&self, position: f64) -> Result<MediaStatus> {
        self.command(json!({"type": "SEEK", "currentTime": position}))
    }

    /// Show these text tracks (none for no subtitles), looking as `style`
    /// says, or as they did.
    pub fn set_tracks(
        &self,
        active: &[u32],
        style: Option<&TextTrackStyle>,
    ) -> Result<MediaStatus> {
        let mut payload = json!({"type": "EDIT_TRACKS_INFO", "activeTrackIds": active});
        if let Some(style) = style {
            payload["textTrackStyle"] = style.json();
        }
        self.command(payload)
    }

    /// What it's doing now, if anything is loaded.
    pub fn status(&self) -> Result<Option<MediaStatus>> {
        let transport = self.transport()?;
        let mut payload = json!({"type": "GET_STATUS"});
        if let Some(media) = self.inner.state().media_session {
            payload["mediaSessionId"] = media.into();
        }
        let answer = self.inner.request(MEDIA, &transport, payload, ANSWER)?;
        Ok(Self::media_answer(&answer).ok())
    }

    /// The device's volume, as it last said.
    pub fn volume(&self) -> Option<Volume> {
        self.inner.state().volume
    }

    fn set_device_volume(&self, volume: Value) -> Result<Volume> {
        let payload = json!({"type": "SET_VOLUME", "volume": volume});
        let answer = self.inner.request(RECEIVER, PLATFORM, payload, ANSWER)?;
        let volume = Volume::read(&answer["status"]["volume"])
            .ok_or_else(|| Error::protocol("a volume answer without the volume"))?;
        self.inner.state().volume = Some(volume);
        Ok(volume)
    }

    /// The device's volume, 0 to 1.
    pub fn set_volume(&self, level: f32) -> Result<Volume> {
        self.set_device_volume(json!({"level": level.clamp(0.0, 1.0)}))
    }

    pub fn set_muted(&self, muted: bool) -> Result<Volume> {
        self.set_device_volume(json!({"muted": muted}))
    }

    /// End the cast: the app closed on the device, and the connection.
    pub fn stop(self) {
        self.inner.stopping.store(true, Ordering::Relaxed);
        let app = self.inner.state().app_session.clone();
        if let Some(app) = app
            && !self.inner.ended.load(Ordering::Relaxed)
        {
            let stop = json!({"type": "STOP", "sessionId": app});
            if let Err(err) = self.inner.request(RECEIVER, PLATFORM, stop, ANSWER) {
                log::debug!("chromecast: stop: {err}");
            }
        }
        // Dropped: the connection closed, and the thread joined.
    }
}

impl Drop for Session {
    /// Leave the device as it is, and close the connection.
    fn drop(&mut self) {
        self.inner.stopping.store(true, Ordering::Relaxed);
        let transport = self.inner.state().transport.clone();
        let close = json!({"type": "CLOSE"});
        if let Some(transport) = transport {
            let _ = self.inner.send(CONNECTION, &transport, &close);
        }
        let _ = self.inner.send(CONNECTION, PLATFORM, &close);
        self.inner.channel.close();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
