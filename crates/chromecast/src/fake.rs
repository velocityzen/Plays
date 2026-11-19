//! A pretend Cast device on this computer, for tests: TLS with a
//! certificate made for it, and the platform and the Default Media
//! Receiver as a device has them. It keeps every message, and ends a cast
//! the ways a device does.

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection};
use serde_json::{Value, json};

use crate::channel::{Channel, provider};
use crate::device::{Device, Record};
use crate::proto::{CONNECTION, CastMessage, HEARTBEAT, MEDIA, PLATFORM, RECEIVER, kind};
use crate::session::DEFAULT_MEDIA_RECEIVER;

/// How often its threads look up to see if it's gone.
const POLL: Duration = Duration::from_millis(20);
/// Who a message to every sender is for.
const EVERYONE: &str = "*";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A Chromecast in a TV.
    Tv,
    /// A speaker, which plays sound only.
    Speaker,
}

/// A message it was sent.
#[derive(Clone, Debug)]
pub struct Received {
    pub namespace: String,
    pub source: String,
    pub destination: String,
    pub payload: Value,
}

/// The app running on it.
#[derive(Clone, Debug)]
pub struct App {
    pub session: String,
    pub transport: String,
}

/// What it's playing.
#[derive(Clone, Debug)]
pub struct Playing {
    pub media_session: u64,
    /// The media as LOAD gave it.
    pub media: Value,
    /// `PLAYING`, `PAUSED` or `IDLE`.
    pub state: &'static str,
    pub idle_reason: Option<&'static str>,
    pub position: f64,
    pub active_tracks: Vec<u32>,
    pub style: Option<Value>,
}

/// What the fake has been sent and is doing.
pub struct State {
    pub received: Vec<Received>,
    pub app: Option<App>,
    pub volume: f64,
    pub muted: bool,
    pub playing: Option<Playing>,
    /// Whether it says no to LOAD.
    pub refuse_load: bool,
    /// Whether it answers nothing, as a device that has hung.
    pub silent: bool,
    /// PONGs it had from senders.
    pub pongs: usize,
    launched: u64,
    media_sessions: u64,
}

impl State {
    /// The messages it was sent of this kind.
    pub fn received(&self, namespace: &str, of: &str) -> Vec<&Received> {
        self.received
            .iter()
            .filter(|message| message.namespace == namespace && kind(&message.payload) == of)
            .collect()
    }

    fn receiver_status(&self, request: &Value) -> Value {
        let applications: Vec<Value> = self
            .app
            .iter()
            .map(|app| {
                json!({
                    "appId": DEFAULT_MEDIA_RECEIVER,
                    "displayName": "Default Media Receiver",
                    "sessionId": app.session,
                    "transportId": app.transport,
                    "namespaces": [{"name": MEDIA}],
                    "statusText": "Ready To Cast",
                })
            })
            .collect();
        json!({
            "type": "RECEIVER_STATUS",
            "requestId": request,
            "status": {
                "applications": applications,
                "volume": {
                    "level": self.volume,
                    "muted": self.muted,
                    "controlType": "attenuation",
                    "stepInterval": 0.05,
                },
            },
        })
    }

    fn media_status(&self, request: &Value) -> Value {
        let status: Vec<Value> = self
            .playing
            .iter()
            .map(|playing| {
                let mut status = json!({
                    "mediaSessionId": playing.media_session,
                    "playbackRate": 1,
                    "playerState": playing.state,
                    "currentTime": playing.position,
                    "activeTrackIds": playing.active_tracks,
                    "media": playing.media,
                });
                if let Some(reason) = playing.idle_reason {
                    status["idleReason"] = reason.into();
                }
                status
            })
            .collect();
        json!({"type": "MEDIA_STATUS", "requestId": request, "status": status})
    }
}

struct Shared {
    kind: Kind,
    state: Mutex<State>,
    gone: AtomicBool,
    connections: Mutex<Vec<Arc<Channel>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn gone(&self) -> bool {
        self.gone.load(Ordering::Relaxed)
    }

    /// Keep a connection to close when it goes away, or close it now if
    /// it's gone.
    fn keep(&self, channel: &Arc<Channel>) {
        lock(&self.connections).push(Arc::clone(channel));
        if self.gone() {
            channel.close();
        }
    }

    /// Say something to every sender.
    fn tell_everyone(&self, source: &str, namespace: &str, payload: &Value) {
        let message = CastMessage::json(source, EVERYONE, namespace, payload);
        for channel in lock(&self.connections).iter() {
            let _ = channel.send(&message);
        }
    }
}

pub struct FakeDevice {
    address: SocketAddr,
    shared: Arc<Shared>,
}

impl FakeDevice {
    pub fn start(kind: Kind) -> Self {
        let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
            .expect("a certificate for the fake");
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()));
        let config = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .expect("TLS 1.2 and 1.3")
            .with_no_client_auth()
            .with_single_cert(vec![made.cert.der().clone()], key)
            .expect("the fake's certificate");
        let config = Arc::new(config);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port for the fake");
        listener
            .set_nonblocking(true)
            .expect("a listener that doesn't block");
        let address = listener.local_addr().expect("the fake's address");
        let shared = Arc::new(Shared {
            kind,
            state: Mutex::new(State {
                received: Vec::new(),
                app: None,
                volume: 0.5,
                muted: false,
                playing: None,
                refuse_load: false,
                silent: false,
                pongs: 0,
                launched: 0,
                media_sessions: 0,
            }),
            gone: AtomicBool::new(false),
            connections: Mutex::default(),
        });
        let accepting = Arc::clone(&shared);
        thread::spawn(move || {
            while !accepting.gone() {
                let Ok((tcp, _)) = listener.accept() else {
                    thread::sleep(POLL);
                    continue;
                };
                let _ = tcp.set_nonblocking(false);
                let shared = Arc::clone(&accepting);
                let config = Arc::clone(&config);
                thread::spawn(move || serve(tcp, config, &shared));
            }
        });
        Self { address, shared }
    }

    /// The device as a browser finds it.
    pub fn device(&self) -> Device {
        let (id, name, model, capabilities) = match self.shared.kind {
            Kind::Tv => (
                "fa4e0000000000000000000000000001",
                "Fake TV",
                "Chromecast",
                "463365",
            ),
            Kind::Speaker => (
                "fa4e0000000000000000000000000002",
                "Fake Speaker",
                "Google Nest Mini",
                "199172",
            ),
        };
        let txt = [
            ("id", id),
            ("fn", name),
            ("md", model),
            ("ca", capabilities),
            ("rs", ""),
        ];
        let record = Record {
            instance: format!("Fake-{id}"),
            addresses: vec![self.address.ip()],
            port: self.address.port(),
            txt: txt
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        };
        Device::from_record(&record).expect("the fake is a device")
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.shared.state()
    }

    /// Wait up to two seconds for `done` to be true of its state.
    pub fn wait_for(&self, done: impl Fn(&State) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if done(&self.state()) {
                return true;
            }
            thread::sleep(POLL);
        }
        false
    }

    /// What it plays comes to its end.
    pub fn finish(&self) {
        let status = {
            let mut state = self.state();
            let Some(playing) = &mut state.playing else {
                return;
            };
            playing.state = "IDLE";
            playing.idle_reason = Some("FINISHED");
            playing.position = playing.media["duration"].as_f64().unwrap_or(0.0);
            state.media_status(&0.into())
        };
        self.tell_from_the_app(MEDIA, &status);
    }

    /// Another sender plays something else in the app.
    pub fn replace(&self) {
        let status = {
            let mut state = self.state();
            state.media_sessions += 1;
            let media_session = state.media_sessions;
            state.playing = Some(Playing {
                media_session,
                media: json!({"contentId": "http://192.0.2.9/other.mp4", "duration": 30.0}),
                state: "PLAYING",
                idle_reason: None,
                position: 0.0,
                active_tracks: Vec::new(),
                style: None,
            });
            state.media_status(&0.into())
        };
        self.tell_from_the_app(MEDIA, &status);
    }

    /// The app closes, as with the remote's home button, which the app's
    /// CLOSE says.
    pub fn close_app(&self) {
        let transport = {
            let mut state = self.state();
            let Some(app) = state.app.take() else {
                return;
            };
            state.playing = None;
            app.transport
        };
        self.shared
            .tell_everyone(&transport, CONNECTION, &json!({"type": "CLOSE"}));
    }

    /// Another sender launches another app in its place, which only the
    /// platform's status says.
    pub fn launch_another_app(&self) {
        let status = {
            let mut state = self.state();
            state.app = None;
            state.playing = None;
            let mut status = state.receiver_status(&0.into());
            status["status"]["applications"] = json!([{
                "appId": "233637DE",
                "displayName": "Another App",
                "sessionId": "another-session",
                "transportId": "another-transport",
            }]);
            status
        };
        self.shared.tell_everyone(PLATFORM, RECEIVER, &status);
    }

    fn tell_from_the_app(&self, namespace: &str, payload: &Value) {
        let transport = self.state().app.as_ref().map(|app| app.transport.clone());
        if let Some(transport) = transport {
            self.shared.tell_everyone(&transport, namespace, payload);
        }
    }

    /// Go off the network: every connection closed, and no more taken.
    pub fn go_away(&self) {
        self.shared.gone.store(true, Ordering::Relaxed);
        for channel in lock(&self.shared.connections).drain(..) {
            channel.close();
        }
    }
}

impl Drop for FakeDevice {
    fn drop(&mut self) {
        self.go_away();
    }
}

/// One sender's connection: TLS, then messages answered until it closes.
fn serve(tcp: TcpStream, config: Arc<ServerConfig>, shared: &Shared) {
    let Ok(tls) = ServerConnection::new(config) else {
        return;
    };
    let Ok((channel, mut incoming)) = Channel::start(tcp, tls.into()) else {
        return;
    };
    let channel = Arc::new(channel);
    shared.keep(&channel);
    let _ = incoming.set_timeout(Some(POLL));
    loop {
        match incoming.next() {
            Ok(Some(message)) => answer(shared, &channel, &message),
            Ok(None) => break,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && !shared.gone() => {}
            Err(_) => break,
        }
    }
}

fn answer(shared: &Shared, channel: &Channel, message: &CastMessage) {
    let payload = message.payload();
    let (from, to) = (message.destination_id.as_str(), message.source_id.as_str());
    let namespace = message.namespace.as_str();
    let mut state = shared.state();
    state.received.push(Received {
        namespace: namespace.to_owned(),
        source: to.to_owned(),
        destination: from.to_owned(),
        payload: payload.clone(),
    });
    if state.silent {
        return;
    }
    let reply = |namespace: &str, payload: Value| {
        let _ = channel.send(&CastMessage::json(from, to, namespace, &payload));
    };
    let request = &payload["requestId"];
    let to_the_app = state.app.as_ref().is_some_and(|app| app.transport == from);
    match (namespace, kind(&payload)) {
        (HEARTBEAT, "PING") => reply(HEARTBEAT, json!({"type": "PONG"})),
        (HEARTBEAT, "PONG") => state.pongs += 1,
        // It pings a sender as it connects, as devices do every so often.
        (CONNECTION, "CONNECT") if from == PLATFORM => reply(HEARTBEAT, json!({"type": "PING"})),
        (RECEIVER, "GET_STATUS") => reply(RECEIVER, state.receiver_status(request)),
        (RECEIVER, "LAUNCH") => {
            if payload["appId"] != DEFAULT_MEDIA_RECEIVER {
                return reply(
                    RECEIVER,
                    json!({"type": "LAUNCH_ERROR", "requestId": request, "reason": "NOT_FOUND"}),
                );
            }
            if state.app.is_none() {
                state.launched += 1;
                state.app = Some(App {
                    session: format!("fake-session-{}", state.launched),
                    transport: format!("fake-transport-{}", state.launched),
                });
            }
            reply(RECEIVER, state.receiver_status(request));
        }
        (RECEIVER, "STOP") => {
            if state.app.as_ref().map(|app| app.session.as_str()) == payload["sessionId"].as_str() {
                state.app = None;
                state.playing = None;
            }
            reply(RECEIVER, state.receiver_status(request));
        }
        (RECEIVER, "SET_VOLUME") => {
            if let Some(level) = payload["volume"]["level"].as_f64() {
                state.volume = level;
            }
            if let Some(muted) = payload["volume"]["muted"].as_bool() {
                state.muted = muted;
            }
            reply(RECEIVER, state.receiver_status(request));
        }
        (MEDIA, command) if to_the_app => {
            let answer = media(&mut state, command, &payload);
            reply(MEDIA, answer);
        }
        _ => {}
    }
}

/// The Default Media Receiver's answer to a media command.
fn media(state: &mut State, command: &str, payload: &Value) -> Value {
    let request = &payload["requestId"];
    let refuse =
        |reason: &str| json!({"type": "INVALID_REQUEST", "requestId": request, "reason": reason});
    if command == "LOAD" {
        if state.refuse_load {
            return json!({"type": "LOAD_FAILED", "requestId": request});
        }
        state.media_sessions += 1;
        let autoplay = payload["autoplay"].as_bool().unwrap_or(true);
        state.playing = Some(Playing {
            media_session: state.media_sessions,
            media: payload["media"].clone(),
            state: if autoplay { "PLAYING" } else { "PAUSED" },
            idle_reason: None,
            position: payload["currentTime"].as_f64().unwrap_or(0.0),
            active_tracks: tracks(&payload["activeTrackIds"]),
            style: payload["media"].get("textTrackStyle").cloned(),
        });
        return state.media_status(request);
    }
    let asked_for = payload["mediaSessionId"].as_u64();
    let Some(playing) = state.playing.as_mut() else {
        return if command == "GET_STATUS" {
            state.media_status(request)
        } else {
            refuse("INVALID_MEDIA_SESSION_ID")
        };
    };
    if asked_for.is_some_and(|id| id != playing.media_session)
        || (asked_for.is_none() && command != "GET_STATUS")
    {
        return refuse("INVALID_MEDIA_SESSION_ID");
    }
    match command {
        "PLAY" => playing.state = "PLAYING",
        "PAUSE" => playing.state = "PAUSED",
        "SEEK" => playing.position = payload["currentTime"].as_f64().unwrap_or(0.0),
        "EDIT_TRACKS_INFO" => {
            playing.active_tracks = tracks(&payload["activeTrackIds"]);
            if let Some(style) = payload.get("textTrackStyle") {
                playing.style = Some(style.clone());
            }
        }
        "GET_STATUS" => {}
        _ => return refuse("INVALID_COMMAND"),
    }
    state.media_status(request)
}

fn tracks(ids: &Value) -> Vec<u32> {
    ids.as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| u32::try_from(id.as_u64()?).ok())
        .collect()
}
