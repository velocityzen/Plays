//! A pretend renderer on this computer, for tests here and in what uses the
//! crate. It answers searches and sends adverts, gives its description,
//! takes SOAP actions on AVTransport, RenderingControl and
//! ConnectionManager, and sends GENA events to whoever subscribes. Its
//! state can be looked at, and changed as from its remote.

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::device::Device;
use crate::http::{self, Message, Url};
use crate::media::{self, Capability};
use crate::soap;
use crate::ssdp::{self, Advert, MEDIA_RENDERER};
use crate::xml::{Element, escape};

const AV_TRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";
const RENDERING_CONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:1";
const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";
/// What it says it plays something else when told to.
pub const SOMETHING_ELSE: &str = "http://192.0.2.99/something-else.mp4";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A TV, with events for its transport and its volume.
    Tv,
    /// A speaker without events, to be asked, that seeks only by absolute
    /// time.
    Speaker,
}

/// An action it was asked to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Call {
    /// The service's type.
    pub service: String,
    pub action: String,
    pub args: Vec<(String, String)>,
}

impl Call {
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Subscriber {
    pub service: String,
    pub sid: String,
    pub callback: String,
    pub renewals: u32,
    seq: u32,
}

#[derive(Debug)]
pub struct State {
    pub calls: Vec<Call>,
    /// What it was given to play, and the DIDL-Lite with it.
    pub uri: String,
    /// The track it says it plays, when that isn't `uri`.
    pub track: Option<String>,
    pub metadata: String,
    /// AVTransport's state, as it says it.
    pub transport: String,
    /// Seconds.
    pub position: f64,
    pub duration: f64,
    /// 0 to 100.
    pub volume: u32,
    pub muted: bool,
    pub subscribers: Vec<Subscriber>,
    /// The SIDs unsubscribed.
    pub unsubscribed: Vec<String>,
    /// Actions it refuses, with UPnP's number.
    pub refuse: Vec<(String, u16)>,
    /// Whether it seeks by REL_TIME, or only ABS_TIME.
    pub rel_time: bool,
    /// Whether it refuses another URI while it plays.
    pub locked_while_playing: bool,
    /// How long it takes to answer an action, after it's done it.
    pub answer_after: Duration,
    /// How long a subscription lasts, in seconds.
    pub subscription: u64,
    /// The max-age its adverts and answers give, in seconds.
    pub max_age: u64,
    /// Whether it answers searches.
    pub answers: bool,
    pub searches: u32,
    /// What it says it plays: ConnectionManager's sink protocol info, a
    /// Samsung TV's or a Sonos speaker's to start with.
    pub sink: String,
    next_sid: u32,
}

impl State {
    /// The calls of `action`.
    pub fn received(&self, action: &str) -> Vec<&Call> {
        self.calls
            .iter()
            .filter(|call| call.action == action)
            .collect()
    }

    fn last_change(&self, service: &str) -> String {
        let instance = if service == AV_TRANSPORT {
            format!(
                "<TransportState val=\"{}\"/><AVTransportURI val=\"{}\"/>\
                 <CurrentTrackURI val=\"{}\"/><CurrentTrackDuration val=\"{}\"/>",
                self.transport,
                escape(&self.uri),
                escape(&self.uri),
                media::clock(self.duration)
            )
        } else {
            format!(
                "<Volume channel=\"Master\" val=\"{}\"/><Volume channel=\"LF\" val=\"0\"/>\
                 <Mute channel=\"Master\" val=\"{}\"/>",
                self.volume,
                u8::from(self.muted)
            )
        };
        let namespace = if service == AV_TRANSPORT {
            "AVT"
        } else {
            "RCS"
        };
        let event = format!(
            "<Event xmlns=\"urn:schemas-upnp-org:metadata-1-0/{namespace}/\">\
             <InstanceID val=\"0\">{instance}</InstanceID></Event>"
        );
        format!(
            "<?xml version=\"1.0\"?><e:propertyset xmlns:e=\"urn:schemas-upnp-org:event-1-0\">\
             <e:property><LastChange>{}</LastChange></e:property></e:propertyset>",
            escape(&event)
        )
    }
}

type Shared = Arc<Mutex<State>>;

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Send `service`'s state to its subscribers, or `only` the one.
fn notify(state: &Mutex<State>, service: &str, only: Option<&str>) {
    let sends: Vec<(String, String, u32, String)> = {
        let mut state = lock(state);
        let body = state.last_change(service);
        state
            .subscribers
            .iter_mut()
            .filter(|subscriber| subscriber.service == service)
            .filter(|subscriber| only.is_none_or(|sid| subscriber.sid == sid))
            .map(|subscriber| {
                let seq = subscriber.seq;
                subscriber.seq += 1;
                (
                    subscriber.callback.clone(),
                    subscriber.sid.clone(),
                    seq,
                    body.clone(),
                )
            })
            .collect()
    };
    for (callback, sid, seq, body) in sends {
        let Some(url) = Url::parse(&callback) else {
            continue;
        };
        let seq = seq.to_string();
        let sent = http::request(
            "NOTIFY",
            &url,
            &[
                ("Content-Type", "text/xml; charset=\"utf-8\""),
                ("NT", "upnp:event"),
                ("NTS", "upnp:propchange"),
                ("SID", &sid),
                ("SEQ", &seq),
            ],
            body.as_bytes(),
            Duration::from_secs(2),
        );
        if let Err(err) = sent {
            log::debug!("fake renderer: notifying: {err}");
        }
    }
}

pub struct FakeRenderer {
    kind: Kind,
    udn: String,
    http: SocketAddr,
    ssdp: SocketAddr,
    state: Shared,
    stopped: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl FakeRenderer {
    pub fn start(kind: Kind) -> Self {
        let state = Arc::new(Mutex::new(State {
            calls: Vec::new(),
            uri: String::new(),
            track: None,
            metadata: String::new(),
            transport: "NO_MEDIA_PRESENT".into(),
            position: 0.0,
            duration: 0.0,
            volume: 20,
            muted: false,
            subscribers: Vec::new(),
            unsubscribed: Vec::new(),
            refuse: Vec::new(),
            rel_time: kind == Kind::Tv,
            locked_while_playing: false,
            answer_after: Duration::ZERO,
            subscription: 300,
            max_age: 1800,
            answers: true,
            searches: 0,
            sink: sink(kind).to_owned(),
            next_sid: 1,
        }));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let http = listener.local_addr().unwrap();
        let ssdp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        ssdp.set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        let ssdp_at = ssdp.local_addr().unwrap();
        let udn = format!("uuid:00000000-0000-1000-8000-{:012x}", http.port());
        let stopped = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        {
            let state = Arc::clone(&state);
            let stopped = Arc::clone(&stopped);
            let udn = udn.clone();
            threads.push(thread::spawn(move || {
                for stream in listener.incoming() {
                    if stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Ok(stream) = stream {
                        serve(kind, &udn, &state, &stream);
                    }
                }
            }));
        }
        {
            let state = Arc::clone(&state);
            let stopped = Arc::clone(&stopped);
            let advert = Self::advert_of(&udn, http);
            threads.push(thread::spawn(move || {
                let mut buffer = [0; 4096];
                while !stopped.load(Ordering::Relaxed) {
                    let Ok((n, from)) = ssdp.recv_from(&mut buffer) else {
                        continue;
                    };
                    let Some(search) = ssdp::Message::parse(&buffer[..n]) else {
                        continue;
                    };
                    let mut state = lock(&state);
                    if state.answers && search.searches_for(MEDIA_RENDERER) {
                        state.searches += 1;
                        let advert = Advert {
                            max_age: Duration::from_secs(state.max_age),
                            ..advert.clone()
                        };
                        let _ = ssdp.send_to(ssdp::answer(&advert).as_bytes(), from);
                    }
                }
            }));
        }
        Self {
            kind,
            udn,
            http,
            ssdp: ssdp_at,
            state,
            stopped,
            threads,
        }
    }

    fn advert_of(udn: &str, http: SocketAddr) -> Advert {
        Advert {
            udn: udn.to_owned(),
            kind: MEDIA_RENDERER.into(),
            location: format!("http://{http}/description.xml"),
            max_age: Duration::from_secs(1800),
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Where its description is.
    pub fn location(&self) -> String {
        format!("http://{}/description.xml", self.http)
    }

    /// As it's found.
    pub fn device(&self) -> Device {
        Device::describe(&self.location()).unwrap()
    }

    /// Where it answers searches.
    pub fn search_at(&self) -> SocketAddr {
        self.ssdp
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Wait up to two seconds for `ready` to hold.
    pub fn wait_for(&self, ready: impl Fn(&State) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if ready(&self.state()) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn send(&self, datagram: &str, to: SocketAddr) {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket.send_to(datagram.as_bytes(), to).unwrap();
    }

    /// Say it's here, to `to`, as it would to SSDP's group.
    pub fn announce(&self, to: SocketAddr) {
        let advert = Advert {
            max_age: Duration::from_secs(self.state().max_age),
            ..Self::advert_of(&self.udn, self.http)
        };
        self.send(&ssdp::alive(&advert), to);
    }

    /// Say it's going, to `to`.
    pub fn byebye(&self, to: SocketAddr) {
        self.send(&ssdp::byebye(&self.udn, MEDIA_RENDERER), to);
    }

    /// Play to the end.
    pub fn finish(&self) {
        {
            let mut state = self.state();
            state.transport = "STOPPED".into();
            state.position = state.duration;
        }
        notify(&self.state, AV_TRANSPORT, None);
    }

    /// Play something else, as from another controller.
    pub fn play_something_else(&self) {
        {
            let mut state = self.state();
            state.uri = SOMETHING_ELSE.into();
            state.transport = "PLAYING".into();
            state.position = 0.0;
        }
        notify(&self.state, AV_TRANSPORT, None);
    }

    /// Turn its volume, as from its remote.
    pub fn turn_volume(&self, volume: u32) {
        self.state().volume = volume;
        notify(&self.state, RENDERING_CONTROL, None);
    }

    /// Stop answering anything, as when it's switched off.
    pub fn go_away(&mut self) {
        if self.stopped.swap(true, Ordering::Relaxed) {
            return;
        }
        let _ = TcpStream::connect(self.http);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for FakeRenderer {
    fn drop(&mut self) {
        self.go_away();
    }
}

fn description(kind: Kind, udn: &str) -> String {
    let service = |name: &str, kind_of: &str, evented: bool| {
        let events = if evented {
            format!("/{name}/event")
        } else {
            String::new()
        };
        format!(
            "<service><serviceType>{kind_of}</serviceType>\
             <serviceId>urn:upnp-org:serviceId:{name}</serviceId>\
             <SCPDURL>/{name}/scpd.xml</SCPDURL><controlURL>/{name}/control</controlURL>\
             <eventSubURL>{events}</eventSubURL></service>"
        )
    };
    let evented = kind == Kind::Tv;
    let (name, model) = match kind {
        Kind::Tv => ("[TV] Fake", "Fake TV"),
        Kind::Speaker => ("Fake Speaker", "Fake Speaker"),
    };
    format!(
        "<?xml version=\"1.0\"?>\
         <root xmlns=\"urn:schemas-upnp-org:device-1-0\">\
         <specVersion><major>1</major><minor>0</minor></specVersion><device>\
         <deviceType>{MEDIA_RENDERER}</deviceType><friendlyName>{name}</friendlyName>\
         <manufacturer>Plays</manufacturer><modelName>{model}</modelName><UDN>{udn}</UDN>\
         <iconList><icon><mimetype>image/png</mimetype><width>120</width><height>120</height>\
         <depth>24</depth><url>/icon.png</url></icon></iconList><serviceList>{}{}{}</serviceList>\
         </device></root>",
        service("AVTransport", AV_TRANSPORT, evented),
        service("RenderingControl", RENDERING_CONTROL, evented),
        service("ConnectionManager", CONNECTION_MANAGER, false),
    )
}

fn sink(kind: Kind) -> &'static str {
    match kind {
        Kind::Tv => include_str!("../fixtures/samsung-protocol-info.txt"),
        Kind::Speaker => include_str!("../fixtures/sonos-protocol-info.txt"),
    }
}

/// What a request was answered with, and the service whose subscribers
/// are to hear of it after.
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
    notify: Option<&'static str>,
    /// The subscription to send its first event to.
    first_event: Option<String>,
}

impl Reply {
    fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: String::new(),
            notify: None,
            first_event: None,
        }
    }

    fn xml(body: String) -> Self {
        Self {
            headers: vec![("Content-Type".into(), "text/xml; charset=\"utf-8\"".into())],
            body,
            ..Self::status(200)
        }
    }
}

fn serve(kind: Kind, udn: &str, state: &Shared, stream: &TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let Ok(message) = Message::read(stream, false) else {
        return;
    };
    let Some((method, path)) = message.head.request() else {
        return;
    };
    let service = match path.split('/').nth(1) {
        Some("AVTransport") => AV_TRANSPORT,
        Some("RenderingControl") => RENDERING_CONTROL,
        Some("ConnectionManager") => CONNECTION_MANAGER,
        _ => "",
    };
    let reply = match (method, path.rsplit('/').next().unwrap_or_default()) {
        ("GET", "description.xml") => Reply::xml(description(kind, udn)),
        ("POST", "control") => {
            let reply = control(service, state, &message);
            let after = lock(state).answer_after;
            thread::sleep(after);
            reply
        }
        ("SUBSCRIBE", "event") if kind == Kind::Tv => subscribe(service, state, &message),
        ("UNSUBSCRIBE", "event") => {
            let mut state = lock(state);
            let sid = message.header("SID").unwrap_or_default();
            let before = state.subscribers.len();
            state.subscribers.retain(|subscriber| subscriber.sid != sid);
            if state.subscribers.len() < before {
                state.unsubscribed.push(sid.to_owned());
                Reply::status(200)
            } else {
                Reply::status(412)
            }
        }
        _ => Reply::status(404),
    };
    let headers: Vec<(&str, &str)> = reply
        .headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let _ = http::respond(stream, reply.status, &headers, reply.body.as_bytes());
    if let Some(service) = reply.notify {
        notify(state, service, None);
    }
    if let Some(sid) = reply.first_event {
        notify(state, service, Some(&sid));
    }
}

fn subscribe(service: &'static str, state: &Shared, message: &Message) -> Reply {
    let mut state = lock(state);
    let timeout = format!("Second-{}", state.subscription);
    if let Some(sid) = message.header("SID") {
        let Some(subscriber) = state
            .subscribers
            .iter_mut()
            .find(|subscriber| subscriber.sid == sid)
        else {
            return Reply::status(412);
        };
        subscriber.renewals += 1;
        return Reply {
            headers: vec![("SID".into(), sid.to_owned()), ("TIMEOUT".into(), timeout)],
            ..Reply::status(200)
        };
    }
    let callback = message
        .header("CALLBACK")
        .and_then(|callback| callback.split('>').next())
        .map(|callback| callback.trim_start_matches('<').to_owned());
    let (Some(callback), Some("upnp:event")) = (callback, message.header("NT")) else {
        return Reply::status(412);
    };
    let sid = format!("uuid:fake-subscription-{}", state.next_sid);
    state.next_sid += 1;
    state.subscribers.push(Subscriber {
        service: service.to_owned(),
        sid: sid.clone(),
        callback,
        renewals: 0,
        seq: 0,
    });
    Reply {
        headers: vec![("SID".into(), sid.clone()), ("TIMEOUT".into(), timeout)],
        first_event: Some(sid),
        ..Reply::status(200)
    }
}

fn control(service: &'static str, shared: &Shared, message: &Message) -> Reply {
    let Some(call) = soap::read_call(message) else {
        return Reply::status(400);
    };
    let action = call.action.clone();
    let mut state = lock(shared);
    state.calls.push(call.clone());
    let refuse = |code: u16, description: &str| Reply {
        status: 500,
        ..Reply::xml(soap::fault(code, description))
    };
    if call.service != service {
        return refuse(401, "Invalid Action");
    }
    if let Some((_, code)) = state.refuse.iter().find(|(refused, _)| *refused == action) {
        return refuse(*code, "Refused");
    }
    let arg = |name: &str| call.arg(name).unwrap_or_default().to_owned();
    let mut notify = None;
    let out: Vec<(&str, String)> = match (service, action.as_str()) {
        (AV_TRANSPORT, "SetAVTransportURI")
            if state.locked_while_playing && state.transport == "PLAYING" =>
        {
            return refuse(705, "Transport is locked");
        }
        (AV_TRANSPORT, "SetAVTransportURI") => {
            state.uri = arg("CurrentURI");
            state.metadata = arg("CurrentURIMetaData");
            state.duration = Element::parse(&state.metadata)
                .ok()
                .and_then(|didl| {
                    didl.find("res")?
                        .attribute("duration")
                        .and_then(media::seconds)
                })
                .unwrap_or(0.0);
            state.position = 0.0;
            state.transport = "STOPPED".into();
            notify = Some(AV_TRANSPORT);
            Vec::new()
        }
        (AV_TRANSPORT, "Play") if state.uri.is_empty() => {
            return refuse(701, "Transition not available");
        }
        (AV_TRANSPORT, "Play") => {
            state.transport = "PLAYING".into();
            notify = Some(AV_TRANSPORT);
            Vec::new()
        }
        (AV_TRANSPORT, "Pause") if state.transport != "PLAYING" => {
            return refuse(701, "Transition not available");
        }
        (AV_TRANSPORT, "Pause") => {
            state.transport = "PAUSED_PLAYBACK".into();
            notify = Some(AV_TRANSPORT);
            Vec::new()
        }
        (AV_TRANSPORT, "Stop") => {
            state.transport = "STOPPED".into();
            state.position = 0.0;
            notify = Some(AV_TRANSPORT);
            Vec::new()
        }
        (AV_TRANSPORT, "Seek") => {
            let unit = arg("Unit");
            if unit != "ABS_TIME" && !(unit == "REL_TIME" && state.rel_time) {
                return refuse(710, "Seek mode not supported");
            }
            let Some(target) = media::seconds(&arg("Target")) else {
                return refuse(711, "Illegal seek target");
            };
            state.position = target;
            Vec::new()
        }
        (AV_TRANSPORT, "GetPositionInfo") => vec![
            ("Track", "1".into()),
            ("TrackDuration", media::clock(state.duration)),
            ("TrackMetaData", state.metadata.clone()),
            (
                "TrackURI",
                state.track.clone().unwrap_or_else(|| state.uri.clone()),
            ),
            // To the millisecond, as Rygel's renderers say it.
            ("RelTime", media::clock_exact(state.position)),
            ("AbsTime", "NOT_IMPLEMENTED".into()),
            ("RelCount", "2147483647".into()),
            ("AbsCount", "2147483647".into()),
        ],
        (AV_TRANSPORT, "GetMediaInfo") => vec![
            ("NrTracks", u8::from(!state.uri.is_empty()).to_string()),
            ("MediaDuration", media::clock(state.duration)),
            ("CurrentURI", state.uri.clone()),
            ("CurrentURIMetaData", state.metadata.clone()),
            ("NextURI", String::new()),
            ("NextURIMetaData", String::new()),
            ("PlayMedium", "NETWORK".into()),
            ("RecordMedium", "NOT_IMPLEMENTED".into()),
            ("WriteStatus", "NOT_IMPLEMENTED".into()),
        ],
        (AV_TRANSPORT, "GetTransportInfo") => vec![
            ("CurrentTransportState", state.transport.clone()),
            ("CurrentTransportStatus", "OK".into()),
            ("CurrentSpeed", "1".into()),
        ],
        (RENDERING_CONTROL, "GetVolume") => vec![("CurrentVolume", state.volume.to_string())],
        (RENDERING_CONTROL, "SetVolume") => {
            let Ok(volume) = arg("DesiredVolume").parse::<u32>() else {
                return refuse(402, "Invalid Args");
            };
            if volume > 100 {
                return refuse(601, "Argument Value Out of Range");
            }
            state.volume = volume;
            notify = Some(RENDERING_CONTROL);
            Vec::new()
        }
        (RENDERING_CONTROL, "GetMute") => {
            vec![("CurrentMute", u8::from(state.muted).to_string())]
        }
        (RENDERING_CONTROL, "SetMute") => {
            state.muted = matches!(arg("DesiredMute").as_str(), "1" | "true");
            notify = Some(RENDERING_CONTROL);
            Vec::new()
        }
        (CONNECTION_MANAGER, "GetProtocolInfo") => {
            let sink: Vec<String> = Capability::parse_list(&state.sink)
                .iter()
                .map(ToString::to_string)
                .collect();
            vec![("Source", String::new()), ("Sink", sink.join(","))]
        }
        _ => return refuse(401, "Invalid Action"),
    };
    let out: Vec<(&str, &str)> = out
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    Reply {
        notify,
        ..Reply::xml(soap::answer(service, &action, &out))
    }
}
