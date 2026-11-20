//! A session with a renderer: what it's to play set with its DIDL-Lite,
//! then played, paused, sought and its volume set, each a SOAP action that
//! waits for its answer. What it does comes from its events, through GENA
//! subscriptions to a listener of the session's own, and from asking where
//! it is every second, which also tells when it's gone; both are told over
//! a channel.

use std::io;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use network::lock;

use crate::device::{Device, Service};
use crate::error::{Error, Result};
use crate::http::{self, Deadline, Message, Url};
use crate::media::{self, Media, Position, Status, TransportState, Volume};
use crate::soap::{self, Answer, Fault};
use crate::xml::Element;

/// How often to ask where it is, and how long it has to say.
#[cfg(not(test))]
const POLL: Duration = Duration::from_secs(1);
#[cfg(not(test))]
const POLL_ANSWER: Duration = Duration::from_secs(3);
#[cfg(test)]
const POLL: Duration = Duration::from_millis(50);
#[cfg(test)]
const POLL_ANSWER: Duration = Duration::from_millis(500);
/// How many polls in a row may go unanswered before it's taken to be gone.
const GONE_AFTER: u32 = 3;
/// Without events for the volume, how many polls go between asking it.
const VOLUME_EVERY: u32 = 5;
/// How long a subscription is asked for, and how long one lasts that
/// doesn't say.
const SUBSCRIBE_FOR: &str = "Second-300";
const SUBSCRIBED: Duration = Duration::from_secs(300);
/// How long a subscription is taken to last, whatever the renderer says;
/// the fake renderer's last a second, to be renewed in a test.
#[cfg(not(test))]
const SHORTEST_SUBSCRIPTION: Duration = Duration::from_secs(30);
#[cfg(test)]
const SHORTEST_SUBSCRIPTION: Duration = Duration::from_secs(1);
const LONGEST_SUBSCRIPTION: Duration = Duration::from_secs(3600);
/// How long an event or an unsubscribing has.
const BRIEF: Duration = Duration::from_secs(2);
/// How often the events listener looks for one, and to stop: a renderer
/// waits up to this for its event to be taken.
#[cfg(not(test))]
const TICK: Duration = Duration::from_millis(50);
#[cfg(test)]
const TICK: Duration = Duration::from_millis(1);

/// What a session says as it goes.
#[derive(Clone, PartialEq, Debug)]
pub enum Event {
    /// What it's doing, whenever that changes.
    Status(Status),
    /// Its volume changed.
    Volume(Volume),
    /// The session is over; nothing more comes.
    Ended(Ended),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    /// The renderer went away, or stopped answering.
    Gone,
    /// It plays something it wasn't given here.
    Replaced,
}

/// The services with events subscribed to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Evented {
    AvTransport,
    RenderingControl,
}

impl Evented {
    fn path(self) -> &'static str {
        match self {
            Self::AvTransport => "AVTransport",
            Self::RenderingControl => "RenderingControl",
        }
    }
}

struct Subscription {
    of: Evented,
    url: Url,
    sid: String,
    renew_at: Instant,
}

#[derive(Default)]
struct State {
    status: Status,
    volume: Option<Volume>,
    /// The URLs given it here, so what it plays can be told from what
    /// something else gave it.
    loaded: Vec<String>,
    /// Counts loads begun and ended, and those under way, so an answer
    /// can be known to be from before one or during one.
    loads: u64,
    loading: u32,
    /// An event said it plays something not given it here; it's asked on
    /// the next poll.
    suspect: bool,
    subscriptions: Vec<Subscription>,
    /// The services an event has come from. One subscribed to is asked
    /// still until one does: its events may never reach us, as when a
    /// firewall stops them.
    heard: Vec<Evented>,
    /// Why it ended, once it has.
    ended: Option<Ended>,
}

struct Inner {
    device: Device,
    state: Mutex<State>,
    events: UnboundedSender<Event>,
    /// Where its events come, without the service's own part.
    callback: String,
    /// Once it's over, by our hand or not; what ends it after isn't news.
    ended: AtomicBool,
    /// How often it's asked where it is, in milliseconds.
    poll_every: AtomicU64,
}

/// A path no other session has.
fn token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!(
        "dlna-{}-{now:08x}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// How long `Second-1800` or `infinite` is.
fn subscribed_for(timeout: Option<&str>) -> Duration {
    let Some(timeout) = timeout else {
        return SUBSCRIBED;
    };
    if timeout.trim().eq_ignore_ascii_case("infinite") {
        return LONGEST_SUBSCRIPTION;
    }
    let seconds = timeout
        .trim()
        .get(..7)
        .filter(|prefix| prefix.eq_ignore_ascii_case("Second-"))
        .and_then(|_| timeout.trim()[7..].parse().ok());
    seconds.map_or(SUBSCRIBED, |seconds| {
        Duration::from_secs(seconds).clamp(SHORTEST_SUBSCRIPTION, LONGEST_SUBSCRIPTION)
    })
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn rendering_control(&self) -> Result<&Service> {
        self.device
            .rendering_control
            .as_ref()
            .ok_or(Error::Unsupported("RenderingControl"))
    }

    fn call(
        &self,
        service: &Service,
        action: &str,
        args: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<Answer> {
        if self.ended.load(Ordering::Relaxed) {
            return Err(Error::Ended);
        }
        let mut all = vec![("InstanceID", "0")];
        all.extend_from_slice(args);
        soap::call(service, action, &all, timeout)
    }

    fn transport(&self, action: &str, args: &[(&str, &str)]) -> Result<Answer> {
        self.call(&self.device.av_transport, action, args, soap::ANSWER)
    }

    fn position(&self, timeout: Duration) -> Result<Position> {
        let answer = self.call(&self.device.av_transport, "GetPositionInfo", &[], timeout)?;
        Ok(Position {
            track_uri: answer
                .get("TrackURI")
                .filter(|uri| !uri.is_empty())
                .map(str::to_owned),
            position: answer.get("RelTime").and_then(media::seconds),
            duration: answer
                .get("TrackDuration")
                .and_then(media::seconds)
                .filter(|duration| *duration > 0.0),
        })
    }

    fn transport_state(&self, timeout: Duration) -> Result<TransportState> {
        let answer = self.call(&self.device.av_transport, "GetTransportInfo", &[], timeout)?;
        Ok(TransportState::parse(
            answer.get("CurrentTransportState").unwrap_or_default(),
        ))
    }

    fn volume(&self, timeout: Duration) -> Result<Volume> {
        let service = self.rendering_control()?;
        let master = [("Channel", "Master")];
        let level = self.call(service, "GetVolume", &master, timeout)?;
        let level: f32 = level
            .get("CurrentVolume")
            .and_then(|level| level.parse().ok())
            .ok_or_else(|| Error::protocol("GetVolume without the volume"))?;
        // Not every renderer has it.
        let muted = self
            .call(service, "GetMute", &master, timeout)
            .ok()
            .and_then(|muted| muted.get("CurrentMute").map(|it| it == "1" || it == "true"))
            .unwrap_or(false);
        Ok(Volume {
            level: (level / 100.0).clamp(0.0, 1.0),
            muted,
        })
    }

    /// Change what it's doing, telling of it if that's news.
    fn update(&self, change: impl FnOnce(&mut Status)) {
        let mut state = self.state();
        let before = state.status.clone();
        change(&mut state.status);
        if state.status != before {
            let _ = self
                .events
                .unbounded_send(Event::Status(state.status.clone()));
        }
    }

    /// Change its volume, telling of it if that's news.
    fn update_volume(&self, change: impl FnOnce(&mut Volume)) {
        let mut state = self.state();
        let mut volume = state.volume.unwrap_or(Volume {
            level: 0.0,
            muted: false,
        });
        change(&mut volume);
        if state.volume != Some(volume) {
            state.volume = Some(volume);
            let _ = self.events.unbounded_send(Event::Volume(volume));
        }
    }

    /// Whether `uri` is something it wasn't given here, once it has been
    /// given something.
    fn foreign(&self, uri: &str) -> bool {
        let uri = uri.trim();
        let state = self.state();
        !uri.is_empty() && !state.loaded.is_empty() && !state.loaded.iter().any(|ours| ours == uri)
    }

    /// Where loads are: none while one is under way.
    fn mark(&self) -> Option<u64> {
        let state = self.state();
        (state.loading == 0).then_some(state.loads)
    }

    /// Whether something else gave it what it plays: what it's set to play,
    /// asked afresh, isn't ours, with no load of ours in between. What it
    /// says it plays may lag behind what it was given, or be news from
    /// before a load.
    fn replaced(&self) -> bool {
        let Some(mark) = self.mark() else {
            return false;
        };
        let current = self.call(&self.device.av_transport, "GetMediaInfo", &[], POLL_ANSWER);
        match current {
            Ok(media) => {
                self.foreign(media.get("CurrentURI").unwrap_or_default())
                    && self.mark() == Some(mark)
            }
            Err(err) => {
                log::debug!("dlna: GetMediaInfo: {err}");
                false
            }
        }
    }

    fn evented(&self, of: Evented) -> bool {
        let state = self.state();
        state.heard.contains(&of)
            && state
                .subscriptions
                .iter()
                .any(|subscription| subscription.of == of)
    }

    /// Ask where it is, and what it's doing when that doesn't come as
    /// events.
    fn poll(&self, polls: u32) -> Result<Option<Ended>> {
        let asked = Instant::now();
        let position = self.position(POLL_ANSWER)?;
        // It said where it was halfway between the asking and the answer.
        let heard = asked + asked.elapsed() / 2;
        let suspect = std::mem::take(&mut self.state().suspect)
            || position
                .track_uri
                .as_deref()
                .is_some_and(|uri| self.foreign(uri));
        if suspect && self.replaced() {
            return Ok(Some(Ended::Replaced));
        }
        let state = if self.evented(Evented::AvTransport) {
            None
        } else {
            Some(self.transport_state(POLL_ANSWER)?)
        };
        self.update(|status| {
            status.heard_at(position.position, heard);
            if position.duration.is_some() {
                status.duration = position.duration;
            }
            if let Some(state) = state {
                status.state = state;
            }
        });
        if polls.is_multiple_of(VOLUME_EVERY)
            && self.device.rendering_control.is_some()
            && !self.evented(Evented::RenderingControl)
        {
            match self.volume(POLL_ANSWER) {
                Ok(volume) => self.update_volume(|it| *it = volume),
                Err(err) => log::debug!("dlna: volume: {err}"),
            }
        }
        Ok(None)
    }

    /// An event's LastChange, from `of`.
    fn changed(&self, of: Evented, last_change: &Element) {
        let instance = last_change
            .children("InstanceID")
            .find(|instance| instance.attribute("val") == Some("0"))
            .or_else(|| last_change.child("InstanceID"));
        let Some(instance) = instance else {
            return;
        };
        let value = |name: &str| {
            instance
                .children(name)
                .find(|value| {
                    value
                        .attribute("channel")
                        .is_none_or(|channel| channel == "Master")
                })
                .and_then(|value| value.attribute("val"))
        };
        match of {
            Evented::AvTransport => {
                let uri = value("AVTransportURI").or(value("CurrentTrackURI"));
                // Asked about on the next poll rather than here, as the
                // renderer may be waiting to send this thread another event.
                if uri.is_some_and(|uri| self.foreign(uri)) {
                    self.state().suspect = true;
                }
                let state = value("TransportState").map(TransportState::parse);
                let duration = value("CurrentTrackDuration")
                    .or(value("CurrentMediaDuration"))
                    .and_then(media::seconds)
                    .filter(|duration| *duration > 0.0);
                let position = value("RelativeTimePosition").and_then(media::seconds);
                self.update(|status| {
                    if let Some(state) = state {
                        status.state = state;
                    }
                    if duration.is_some() {
                        status.duration = duration;
                    }
                    if position.is_some() {
                        status.heard_at(position, Instant::now());
                    }
                });
            }
            Evented::RenderingControl => {
                let level = value("Volume").and_then(|level| level.parse::<f32>().ok());
                let muted = value("Mute").map(|muted| muted == "1" || muted == "true");
                if level.is_some() || muted.is_some() {
                    self.update_volume(|volume| {
                        if let Some(level) = level {
                            volume.level = (level / 100.0).clamp(0.0, 1.0);
                        }
                        if let Some(muted) = muted {
                            volume.muted = muted;
                        }
                    });
                }
            }
        }
    }

    /// A NOTIFY from the renderer.
    fn event(&self, of: Evented, message: &Message) {
        let Ok(body) = Element::parse(&message.text()) else {
            return;
        };
        let Some(last_change) = body
            .children("property")
            .find_map(|property| property.child("LastChange"))
        else {
            return;
        };
        {
            let mut state = self.state();
            if !state.heard.contains(&of) {
                state.heard.push(of);
            }
        }
        match Element::parse(last_change.text()) {
            Ok(last_change) => self.changed(of, &last_change),
            Err(err) => log::debug!("dlna: an event's LastChange: {err}"),
        }
    }

    fn subscribe(&self, of: Evented, events: &str) -> Result<Subscription> {
        let url = Url::parse(events)
            .ok_or_else(|| Error::protocol(format!("an event URL {events:?}")))?;
        let callback = format!("<{}/{}>", self.callback, of.path());
        let response = http::request(
            "SUBSCRIBE",
            &url,
            &[
                ("CALLBACK", &callback),
                ("NT", "upnp:event"),
                ("TIMEOUT", SUBSCRIBE_FOR),
            ],
            &[],
            soap::ANSWER,
        )?;
        let status = response.head.status().unwrap_or_default();
        let sid = response
            .header("SID")
            .filter(|_| status == 200)
            .ok_or(Error::Http {
                request: "SUBSCRIBE".into(),
                status,
            })?;
        Ok(Subscription {
            of,
            sid: sid.to_owned(),
            renew_at: Instant::now() + subscribed_for(response.header("TIMEOUT")) / 2,
            url,
        })
    }

    fn renew(&self, subscription: &mut Subscription) -> Result<()> {
        let response = http::request(
            "SUBSCRIBE",
            &subscription.url,
            &[("SID", &subscription.sid), ("TIMEOUT", SUBSCRIBE_FOR)],
            &[],
            soap::ANSWER,
        )?;
        let status = response.head.status().unwrap_or_default();
        if status != 200 {
            return Err(Error::Http {
                request: "SUBSCRIBE".into(),
                status,
            });
        }
        if let Some(sid) = response.header("SID") {
            sid.clone_into(&mut subscription.sid);
        }
        subscription.renew_at = Instant::now() + subscribed_for(response.header("TIMEOUT")) / 2;
        Ok(())
    }

    /// Renew the subscriptions about to run out; one that can't be, nor
    /// made again, is left to polling.
    fn renew_due(&self) {
        let now = Instant::now();
        let due: Vec<Subscription> = {
            let mut state = self.state();
            let (due, rest) = std::mem::take(&mut state.subscriptions)
                .into_iter()
                .partition(|subscription| subscription.renew_at <= now);
            state.subscriptions = rest;
            due
        };
        for mut subscription in due {
            if self.ended.load(Ordering::Relaxed) {
                // Left for the session's end to end.
                self.state().subscriptions.push(subscription);
                continue;
            }
            let renewed = self.renew(&mut subscription).or_else(|err| {
                log::debug!("dlna: renewing: {err}");
                let events = subscription.url.to_string();
                self.subscribe(subscription.of, &events)
                    .map(|again| subscription = again)
            });
            match renewed {
                Ok(()) => self.state().subscriptions.push(subscription),
                Err(err) => log::info!("dlna: no more events, polling: {err}"),
            }
        }
    }

    fn unsubscribe(&self) {
        let subscriptions = std::mem::take(&mut self.state().subscriptions);
        for subscription in subscriptions {
            let gone = http::request(
                "UNSUBSCRIBE",
                &subscription.url,
                &[("SID", &subscription.sid)],
                &[],
                BRIEF,
            );
            if let Err(err) = gone {
                log::debug!("dlna: unsubscribing: {err}");
            }
        }
    }

    fn end(&self, why: Ended) {
        if self.ended.swap(true, Ordering::Relaxed) {
            return;
        }
        self.state().ended = Some(why);
        log::info!("dlna: the session ended: {why:?}");
        let _ = self.events.unbounded_send(Event::Ended(why));
    }
}

/// Take the renderer's events until `closing`.
fn listen(inner: &Inner, listener: &TcpListener, closing: &AtomicBool) {
    let base = Url::parse(&inner.callback)
        .map(|url| url.path)
        .unwrap_or_default();
    let path = |of: Evented| format!("{base}/{}", of.path());
    let transport = path(Evented::AvTransport);
    let rendering = path(Evented::RenderingControl);
    // Not waiting in accept, which nothing may wake: once the Mac changes
    // network, nothing can connect to the address it listens at.
    if let Err(err) = listener.set_nonblocking(true) {
        log::info!("dlna: no events: {err}");
        return;
    }
    while !closing.load(Ordering::Relaxed) {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(err) => {
                if !matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) {
                    log::debug!("dlna: events: {err}");
                }
                thread::sleep(TICK);
                continue;
            }
        };
        // Taken as the listener is, on macOS.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        // Events are heard one at a time, so each has BRIEF as a whole:
        // one sent a byte at a time would hold up the rest.
        let _ = stream.set_write_timeout(Some(BRIEF));
        let Ok(message) = Message::read(Deadline::new(&stream, BRIEF), false) else {
            continue;
        };
        let of = match message.head.request() {
            Some(("NOTIFY", at)) if at == transport => Some(Evented::AvTransport),
            Some(("NOTIFY", at)) if at == rendering => Some(Evented::RenderingControl),
            _ => None,
        };
        let status = if of.is_some() { 200 } else { 412 };
        let _ = http::respond(&stream, status, &[], &[]);
        drop(stream);
        if let Some(of) = of
            && !inner.ended.load(Ordering::Relaxed)
        {
            inner.event(of, &message);
        }
    }
}

/// Ask where it is every `POLL`, or as often as asked, and at once when
/// woken, until the waker goes or the session's over.
fn poll(inner: &Inner, wake: &mpsc::Receiver<()>) {
    let mut unanswered = 0;
    for polls in 0u32.. {
        let every = Duration::from_millis(inner.poll_every.load(Ordering::Relaxed));
        match wake.recv_timeout(every) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if inner.ended.load(Ordering::Relaxed) {
            return;
        }
        inner.renew_due();
        match inner.poll(polls) {
            Ok(None) => unanswered = 0,
            Ok(Some(ended)) => return inner.end(ended),
            Err(err) if err.is_unreachable() => {
                log::debug!("dlna: polling: {err}");
                unanswered += 1;
                if unanswered >= GONE_AFTER {
                    log::info!("dlna: the renderer stopped answering");
                    return inner.end(Ended::Gone);
                }
            }
            Err(err) => {
                log::debug!("dlna: polling: {err}");
                unanswered = 0;
            }
        }
    }
}

/// A session with a renderer.
pub struct Session {
    inner: Arc<Inner>,
    closing: Arc<AtomicBool>,
    /// Wakes the polling thread, which stops once it's dropped.
    polling: Option<mpsc::Sender<()>>,
    listener: Option<JoinHandle<()>>,
    poller: Option<JoinHandle<()>>,
}

impl Session {
    /// Start one with `device`: its events subscribed to where it has
    /// them, and asked where it is from now on.
    pub fn connect(device: &Device) -> Result<(Self, UnboundedReceiver<Event>)> {
        let control = Url::parse(&device.av_transport.control).ok_or_else(|| {
            Error::protocol(format!("a control URL {:?}", device.av_transport.control))
        })?;
        let remote = control
            .addresses()?
            .first()
            .ok_or_else(|| Error::protocol(format!("no address for {}", control.host)))?
            .ip();
        // Events come to the address the renderer is reached from.
        let listener = TcpListener::bind((http::local_address_towards(remote)?, 0))?;
        let listening = listener.local_addr()?;
        let (events, receiver) = unbounded();
        let inner = Arc::new(Inner {
            device: device.clone(),
            state: Mutex::default(),
            events,
            callback: format!("http://{listening}/{}", token()),
            ended: AtomicBool::new(false),
            poll_every: AtomicU64::new(POLL.as_millis() as u64),
        });
        let closing = Arc::new(AtomicBool::new(false));
        let listened = Arc::clone(&inner);
        let closed = Arc::clone(&closing);
        let listener = thread::Builder::new()
            .name("dlna events".into())
            .spawn(move || listen(&listened, &listener, &closed))?;
        let services = [
            (Evented::AvTransport, Some(&device.av_transport)),
            (Evented::RenderingControl, device.rendering_control.as_ref()),
        ];
        for (of, service) in services {
            let Some(events) = service.and_then(|service| service.events.as_deref()) else {
                continue;
            };
            match inner.subscribe(of, events) {
                Ok(subscription) => inner.state().subscriptions.push(subscription),
                Err(err) => log::info!("dlna: no events from {}, polling: {err}", of.path()),
            }
        }
        let (polling, wake) = mpsc::channel();
        let polled = Arc::clone(&inner);
        let poller = thread::Builder::new()
            .name("dlna poll".into())
            .spawn(move || poll(&polled, &wake));
        let poller = match poller {
            Ok(poller) => poller,
            Err(err) => {
                closing.store(true, Ordering::Relaxed);
                let _ = listener.join();
                inner.unsubscribe();
                return Err(err.into());
            }
        };
        Ok((
            Self {
                inner,
                closing,
                polling: Some(polling),
                listener: Some(listener),
                poller: Some(poller),
            },
            receiver,
        ))
    }

    pub fn device(&self) -> &Device {
        &self.inner.device
    }

    /// Ask where it is now, and this often from then on, as for a picture
    /// that follows its sound; `None` goes back to once a second.
    pub fn poll_every(&self, every: Option<Duration>) {
        let every = every.unwrap_or(POLL).max(Duration::from_millis(10));
        self.inner
            .poll_every
            .store(every.as_millis() as u64, Ordering::Relaxed);
        if let Some(polling) = &self.polling {
            let _ = polling.send(());
        }
    }

    /// Give it `media` to play, as play then plays.
    pub fn load(&self, media: &Media) -> Result<()> {
        {
            let mut state = self.inner.state();
            state.loaded.push(media.url.trim().to_owned());
            state.loads += 1;
            state.loading += 1;
        }
        let loaded = self.set_uri(media);
        let mut state = self.inner.state();
        state.loading -= 1;
        state.loads += 1;
        loaded
    }

    fn set_uri(&self, media: &Media) -> Result<()> {
        let didl = media.didl();
        let args = [
            ("CurrentURI", media.url.as_str()),
            ("CurrentURIMetaData", didl.as_str()),
        ];
        match self.inner.transport("SetAVTransportURI", &args) {
            // Some won't take another while they play.
            Err(Error::Upnp {
                fault: Fault::TransitionNotAvailable | Fault::TransportLocked,
                ..
            }) => {
                if let Err(err) = self.inner.transport("Stop", &[]) {
                    log::debug!("dlna: stopping to load: {err}");
                }
                self.inner.transport("SetAVTransportURI", &args).map(drop)
            }
            done => done.map(drop),
        }
    }

    pub fn play(&self) -> Result<()> {
        self.inner.transport("Play", &[("Speed", "1")]).map(drop)
    }

    pub fn pause(&self) -> Result<()> {
        self.inner.transport("Pause", &[]).map(drop)
    }

    /// Go to `position` seconds in.
    pub fn seek(&self, position: f64) -> Result<()> {
        let target = media::clock(position);
        match self
            .inner
            .transport("Seek", &[("Unit", "REL_TIME"), ("Target", &target)])
        {
            Err(Error::Upnp {
                fault: Fault::SeekModeNotSupported,
                ..
            }) => self
                .inner
                .transport("Seek", &[("Unit", "ABS_TIME"), ("Target", &target)])
                .map(drop),
            done => done.map(drop),
        }
    }

    /// Where it is now, as it says.
    pub fn position(&self) -> Result<Position> {
        self.inner.position(soap::ANSWER)
    }

    /// What it's doing now, as it says.
    pub fn transport_state(&self) -> Result<TransportState> {
        self.inner.transport_state(soap::ANSWER)
    }

    /// What it's doing, as last told.
    pub fn status(&self) -> Status {
        self.inner.state().status.clone()
    }

    /// Its volume now, as it says.
    pub fn volume(&self) -> Result<Volume> {
        let volume = self.inner.volume(soap::ANSWER)?;
        self.inner.update_volume(|it| *it = volume);
        Ok(volume)
    }

    /// Its volume, 0 to 1.
    pub fn set_volume(&self, level: f32) -> Result<()> {
        let level = level.clamp(0.0, 1.0);
        let percent = ((level * 100.0).round() as u32).to_string();
        self.inner.call(
            self.inner.rendering_control()?,
            "SetVolume",
            &[("Channel", "Master"), ("DesiredVolume", &percent)],
            soap::ANSWER,
        )?;
        self.inner.update_volume(|volume| volume.level = level);
        Ok(())
    }

    pub fn set_muted(&self, muted: bool) -> Result<()> {
        self.inner.call(
            self.inner.rendering_control()?,
            "SetMute",
            &[
                ("Channel", "Master"),
                ("DesiredMute", if muted { "1" } else { "0" }),
            ],
            soap::ANSWER,
        )?;
        self.inner.update_volume(|volume| volume.muted = muted);
        Ok(())
    }

    /// Stop it playing, and end the session.
    pub fn stop(self) {
        if let Err(err) = self.inner.transport("Stop", &[]) {
            log::debug!("dlna: stop: {err}");
        }
        // Dropped: unsubscribed, and the threads joined.
    }
}

impl Drop for Session {
    /// Leave the renderer as it is, unsubscribed.
    fn drop(&mut self) {
        self.inner.ended.store(true, Ordering::Relaxed);
        self.closing.store(true, Ordering::Relaxed);
        drop(self.polling.take());
        // A renewal under way puts its subscription back when answered, so
        // the polling thread is done before they're ended.
        if let Some(poller) = self.poller.take() {
            let _ = poller.join();
        }
        // One gone can't be told.
        if self.inner.state().ended != Some(Ended::Gone) {
            self.inner.unsubscribe();
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_long_a_subscription_lasts() {
        assert_eq!(
            subscribed_for(Some("Second-1800")),
            Duration::from_secs(1800)
        );
        assert_eq!(subscribed_for(Some("second-60 ")), Duration::from_secs(60));
        assert_eq!(subscribed_for(Some("infinite")), Duration::from_secs(3600));
        assert_eq!(subscribed_for(Some("Second-x")), SUBSCRIBED);
        assert_eq!(subscribed_for(None), SUBSCRIBED);
        assert_eq!(
            subscribed_for(Some("Second-18446744073709551615")),
            LONGEST_SUBSCRIPTION,
            "an hour at most"
        );
        assert_eq!(
            subscribed_for(Some("Second-0")),
            SHORTEST_SUBSCRIPTION,
            "not renewed on every poll"
        );
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(token(), token());
    }

    /// The events listener stops once the session closes, with nothing
    /// connecting to it: when the Mac changes network, the address it
    /// listens at goes, and nothing can.
    #[test]
    fn the_events_listener_stops_without_being_woken() {
        let fake = crate::fake::FakeRenderer::start(crate::fake::Kind::Tv);
        let (events, _events) = unbounded();
        let inner = Arc::new(Inner {
            device: fake.device(),
            state: Mutex::default(),
            events,
            callback: format!("http://127.0.0.1:1/{}", token()),
            ended: AtomicBool::new(false),
            poll_every: AtomicU64::new(POLL.as_millis() as u64),
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let closing = Arc::new(AtomicBool::new(false));
        let (stopped, stops) = mpsc::channel();
        let closed = Arc::clone(&closing);
        thread::spawn(move || {
            listen(&inner, &listener, &closed);
            let _ = stopped.send(());
        });
        thread::sleep(Duration::from_millis(50));
        closing.store(true, Ordering::Relaxed);
        assert!(
            stops.recv_timeout(Duration::from_secs(2)).is_ok(),
            "the listener still waits"
        );
    }
}
