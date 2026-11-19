//! A pretend receiver on this computer, for tests: a TV that pairs with a
//! PIN, an AirPlay 2 speaker open to everyone, or an AirPlay 1 speaker,
//! plain or encrypted. It pairs, encrypts, answers as a receiver does,
//! keeps every request, and turns the sound it's sent back into samples.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use plist::{Dictionary, Value};

use crate::channel::{Keys, Sealed};
use crate::connection::Link;
use crate::crypto;
use crate::device::{Device, Record};
use crate::message::{Message, Reader};
use crate::pairing::receiver::{Accessory, Outcome};
use crate::rsa::PrivateKey;
use crate::rtp::{self, receiver};

/// How often its threads look up to see if it's gone.
const POLL: Duration = Duration::from_millis(20);

/// What the fake is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// An Apple TV, which pairs once with this PIN.
    Tv { pin: &'static str },
    /// An AirPlay 2 speaker, open to everyone: transient pairing.
    Speaker,
    /// An AirPlay 1 speaker: no pairing, unencrypted sound.
    OldSpeaker,
    /// An AirPlay 1 speaker that takes sound only encrypted, and doesn't say
    /// its features, as the first AirPort Express: the key comes wrapped
    /// with the fakes' own RSA key, which senders here know them by, rather
    /// than the AirPort's.
    EncryptedSpeaker,
}

impl Kind {
    fn airplay2(self) -> bool {
        !matches!(self, Self::OldSpeaker | Self::EncryptedSpeaker)
    }
}

/// Where the fakes listen, so a sender here wraps keys for them with their
/// key.
static LISTENING: Mutex<Vec<SocketAddr>> = Mutex::new(Vec::new());

/// Whether a fake listens at `address`.
pub(crate) fn listens_at(address: SocketAddr) -> bool {
    lock(&LISTENING).contains(&address)
}

/// How sound comes to it.
#[derive(Clone, Copy, Debug)]
enum Encryption {
    None,
    /// AirPlay 2's: sealed with this key.
    Sealed([u8; 32]),
    /// AirPlay 1's: AES-128 CBC, each packet from the IV.
    Aes {
        key: [u8; crypto::BLOCK],
        iv: [u8; crypto::BLOCK],
    },
}

/// A request the fake was sent.
#[derive(Clone, Debug)]
pub struct Received {
    pub method: String,
    /// The URI as sent, query and all.
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Whether it came encrypted.
    pub sealed: bool,
}

impl Received {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The URI without its query.
    pub fn path(&self) -> &str {
        self.uri.split('?').next().unwrap_or_default()
    }

    /// A number in the query.
    pub fn query(&self, key: &str) -> Option<f64> {
        let (_, query) = self.uri.split_once('?')?;
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(name, _)| *name == key)
            .and_then(|(_, value)| value.parse().ok())
    }

    /// The body, a property list dictionary.
    pub fn dictionary(&self) -> Dictionary {
        Value::from_reader(std::io::Cursor::new(&self.body))
            .ok()
            .and_then(Value::into_dictionary)
            .unwrap_or_default()
    }
}

/// An audio packet it took.
#[derive(Clone, Debug)]
pub struct Packet {
    pub seq: u16,
    pub samples: Vec<i16>,
}

/// What the fake has been sent and says.
pub struct State {
    pub requests: Vec<Received>,
    /// Whether it showed its PIN.
    pub pin_shown: bool,
    /// What `/playback-info` answers.
    pub playback: Dictionary,
    /// How long what it's told to play is, in seconds.
    pub duration: f64,
    pub volume_db: f32,
    /// The delay it says it adds, in frames (AirPlay 1).
    pub audio_latency: u32,
    /// Audio packets by RTP time.
    pub packets: BTreeMap<u32, Packet>,
    /// The sequence numbers of packets sent again.
    pub resent: Vec<u16>,
    /// Sync packets: RTP time less latency, NTP time, RTP time.
    pub syncs: Vec<(u32, u64, u32)>,
    /// The time it asked the sender's clock at, and the answer.
    pub timing: Option<(u64, Vec<u8>)>,
    /// The status of the answer to what it said on the event connection.
    pub event_reply: Option<u16>,
    /// Whether it refuses feedback, as a receiver that has lost the session
    /// does.
    pub refuse_feedback: bool,
    /// Where the sender takes resend requests, and the socket to ask from.
    resends: Option<(SocketAddr, Arc<UdpSocket>)>,
    /// How AirPlay 1's ANNOUNCE said sound comes.
    announced: Encryption,
}

impl State {
    /// The samples it was sent, in order.
    pub fn samples(&self) -> Vec<i16> {
        self.packets
            .values()
            .flat_map(|packet| packet.samples.iter().copied())
            .collect()
    }

    /// The requests with this method and path, query aside.
    pub fn requests(&self, method: &str, path: &str) -> Vec<&Received> {
        self.requests
            .iter()
            .filter(|request| request.method == method && request.path() == path)
            .collect()
    }
}

struct Shared {
    kind: Kind,
    state: Mutex<State>,
    accessory: Mutex<Accessory>,
    gone: AtomicBool,
    /// Its connections, to close when it goes away.
    streams: Mutex<Vec<TcpStream>>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Keep a connection to close when it goes away, or close it now if
    /// it's gone.
    fn keep(&self, stream: &TcpStream) {
        if let Ok(stream) = stream.try_clone() {
            lock(&self.streams).push(stream);
        }
        if self.gone() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    fn gone(&self) -> bool {
        self.gone.load(Ordering::Relaxed)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct FakeReceiver {
    address: SocketAddr,
    shared: Arc<Shared>,
}

impl FakeReceiver {
    pub fn start(kind: Kind) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port for the fake");
        listener
            .set_nonblocking(true)
            .expect("a listener that doesn't block");
        let address = listener.local_addr().expect("the fake's address");
        let pin = match kind {
            Kind::Tv { pin } => pin,
            Kind::Speaker | Kind::OldSpeaker | Kind::EncryptedSpeaker => "",
        };
        let shared = Arc::new(Shared {
            kind,
            state: Mutex::new(State {
                requests: Vec::new(),
                pin_shown: false,
                playback: Dictionary::new(),
                duration: 60.0,
                volume_db: -15.0,
                audio_latency: 0,
                packets: BTreeMap::new(),
                resent: Vec::new(),
                syncs: Vec::new(),
                timing: None,
                event_reply: None,
                refuse_feedback: false,
                resends: None,
                announced: Encryption::None,
            }),
            accessory: Mutex::new(Accessory::new(id(kind), pin)),
            gone: AtomicBool::new(false),
            streams: Mutex::default(),
        });
        let accepting = Arc::clone(&shared);
        thread::spawn(move || {
            while !accepting.gone() {
                let Ok((stream, _)) = listener.accept() else {
                    thread::sleep(POLL);
                    continue;
                };
                let _ = stream.set_nonblocking(false);
                accepting.keep(&stream);
                let shared = Arc::clone(&accepting);
                thread::spawn(move || serve(stream, &shared));
            }
        });
        lock(&LISTENING).push(address);
        Self { address, shared }
    }

    /// The receiver as a browser finds it.
    pub fn device(&self) -> Device {
        let kind = self.shared.kind;
        let record = |instance: String, txt: &str| Record {
            instance,
            addresses: vec![self.address.ip()],
            port: self.address.port(),
            txt: txt
                .split(' ')
                .filter_map(|pair| pair.split_once('='))
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect::<HashMap<_, _>>(),
        };
        let id = id(kind);
        let raop_name = |name: &str| format!("{}@{name}", id.replace(':', ""));
        let device = match kind {
            Kind::Tv { .. } => Device::from_records(
                Some(&record(
                    "Fake TV".into(),
                    &format!(
                        "deviceid={id} features=0x4A7FDFD5,0x3C177FDE flags=0x644 model=AppleTV11,1"
                    ),
                )),
                Some(&record(raop_name("Fake TV"), "cn=0,1,2,3 et=0,3,5")),
            ),
            Kind::Speaker => Device::from_records(
                Some(&record(
                    "Fake Speaker".into(),
                    &format!(
                        "deviceid={id} features=0x445F8A00,0x1C340 flags=0x4 model=Speaker1,1"
                    ),
                )),
                None,
            ),
            Kind::OldSpeaker => Device::from_records(
                None,
                Some(&record(
                    raop_name("Fake Old Speaker"),
                    "ft=0x200 sf=0x0 am=AirPort10,115 cn=0,1 et=0,4",
                )),
            ),
            Kind::EncryptedSpeaker => Device::from_records(
                None,
                Some(&record(
                    raop_name("Fake AirPort"),
                    "txtvers=1 ch=2 cn=0,1 et=1 sr=44100 ss=16 tp=UDP am=AirPort4,107",
                )),
            ),
        };
        device.expect("the fake is a receiver")
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.shared.state()
    }

    /// The ids of the senders paired with its PIN.
    pub fn paired(&self) -> Vec<String> {
        lock(&self.shared.accessory).paired()
    }

    /// Ask the sender for packets again, as a receiver missing them does.
    pub fn ask_again(&self, first: u16, count: u16) {
        let (to, socket) = self.state().resends.clone().expect("a stream set up");
        socket
            .send_to(&receiver::resend_request(first, count), to)
            .expect("a resend request sent");
    }

    /// Go off the network: every connection closed, and no more taken.
    pub fn go_away(&self) {
        self.shared.gone.store(true, Ordering::Relaxed);
        for stream in lock(&self.shared.streams).drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
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
}

impl Drop for FakeReceiver {
    fn drop(&mut self) {
        self.go_away();
        lock(&LISTENING).retain(|address| *address != self.address);
    }
}

/// Made up, and not anyone's.
fn id(kind: Kind) -> &'static str {
    match kind {
        Kind::Tv { .. } => "02:00:00:00:00:01",
        Kind::Speaker => "02:00:00:00:00:02",
        Kind::OldSpeaker => "02:00:00:00:00:03",
        Kind::EncryptedSpeaker => "02:00:00:00:00:04",
    }
}

/// An answer: status, headers beyond the usual, body, and the secret to
/// encrypt with from the next request on.
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    secret: Option<Vec<u8>>,
}

impl Reply {
    fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            secret: None,
        }
    }

    fn ok() -> Self {
        Self::status(200)
    }

    fn body(mut self, content_type: &str, body: Vec<u8>) -> Self {
        self.headers
            .push(("Content-Type".into(), content_type.into()));
        self.body = body;
        self
    }

    fn header(mut self, name: &str, value: impl ToString) -> Self {
        self.headers.push((name.into(), value.to_string()));
        self
    }

    fn plist(self, dictionary: Dictionary) -> Self {
        let mut body = Vec::new();
        plist::to_writer_binary(&mut body, &Value::Dictionary(dictionary))
            .expect("a property list writes");
        self.body("application/x-apple-binary-plist", body)
    }
}

fn int(n: u64) -> Value {
    Value::Integer(n.into())
}

/// One connection: requests in, answers out, encrypted once paired.
fn serve(stream: TcpStream, shared: &Arc<Shared>) {
    let mut link = Link::Plain(stream);
    let mut reader = Reader::default();
    // The session's secret, for the event connection's keys.
    let mut secret: Option<Vec<u8>> = None;
    while let Ok(Some(request)) = reader.read(&mut link) {
        let (method, uri, version) = request.words();
        let received = Received {
            method: method.to_owned(),
            uri: uri.to_owned(),
            headers: request.headers.clone(),
            body: request.body.clone(),
            sealed: matches!(link, Link::Sealed(_)),
        };
        shared.state().requests.push(received.clone());
        let reply = answer(shared, &received, secret.as_deref());
        let mut headers = vec![("Content-Length".to_owned(), reply.body.len().to_string())];
        if let Some(cseq) = request.header("CSeq") {
            headers.push(("CSeq".into(), cseq.into()));
        }
        headers.extend(reply.headers);
        let text = if reply.status < 300 { "OK" } else { "Refused" };
        let message = Message {
            start: format!("{version} {} {text}", reply.status),
            headers,
            body: reply.body,
        };
        if link.write_all(&message.encode()).is_err() {
            break;
        }
        if let Some(paired) = reply.secret {
            let Link::Plain(stream) = link else { break };
            link = Link::Sealed(Sealed::new(stream, Keys::control(&paired).swapped()));
            secret = Some(paired);
        }
    }
}

fn answer(shared: &Arc<Shared>, request: &Received, secret: Option<&[u8]>) -> Reply {
    let pairing = |result: crate::Result<(Vec<u8>, Outcome)>| match result {
        Ok((body, outcome)) => {
            let mut reply = Reply::ok().body("application/octet-stream", body);
            if let Outcome::Secret(secret) = outcome {
                reply.secret = Some(secret);
            }
            reply
        }
        Err(_) => Reply::status(470),
    };
    let kind = shared.kind;
    match (request.method.as_str(), request.path()) {
        ("POST", "/pair-pin-start") => {
            shared.state().pin_shown = true;
            Reply::ok()
        }
        ("POST", "/pair-setup") => {
            let transient = request.header("X-Apple-HKP") == Some("4");
            if transient && matches!(kind, Kind::Tv { .. }) {
                // A TV that wants its PIN doesn't pair for a session only.
                return Reply::status(470);
            }
            pairing(lock(&shared.accessory).setup(&request.body))
        }
        ("POST", "/pair-verify") => pairing(lock(&shared.accessory).verify(&request.body)),
        // An AirPlay 2 receiver takes nothing else until paired.
        _ if !request.sealed && kind.airplay2() => Reply::status(403),
        ("ANNOUNCE", _) => announce(shared, request),
        ("SETUP", _) if request.header("Transport").is_some() => setup_v1(shared, request),
        ("SETUP", _) => setup(shared, request, secret),
        ("SET_PARAMETER", _) => {
            let text = String::from_utf8_lossy(&request.body);
            if let Some(db) = text
                .strip_prefix("volume:")
                .and_then(|db| db.trim().parse().ok())
            {
                shared.state().volume_db = db;
            }
            Reply::ok()
        }
        ("GET_PARAMETER", _) => {
            let db = shared.state().volume_db;
            Reply::ok().body(
                "text/parameters",
                format!("volume: {db:.6}\r\n").into_bytes(),
            )
        }
        ("POST", "/play") => {
            let body = request.dictionary();
            let mut state = shared.state();
            let mut playback = Dictionary::new();
            playback.insert("duration".into(), state.duration.into());
            let start = body.get("Start-Position-Seconds").cloned();
            playback.insert("position".into(), start.unwrap_or(0.0.into()));
            let rate = body.get("rate").cloned();
            playback.insert("rate".into(), rate.unwrap_or(1.0.into()));
            playback.insert("readyToPlay".into(), true.into());
            state.playback = playback;
            Reply::ok()
        }
        ("POST", "/rate") => {
            let rate = request.query("value").unwrap_or(0.0);
            shared.state().playback.insert("rate".into(), rate.into());
            Reply::ok()
        }
        ("POST", "/scrub") => {
            let position = request.query("position").unwrap_or(0.0);
            shared
                .state()
                .playback
                .insert("position".into(), position.into());
            Reply::ok()
        }
        ("POST", "/stop") => {
            shared.state().playback = Dictionary::new();
            Reply::ok()
        }
        ("GET", "/playback-info") => Reply::ok().plist(shared.state().playback.clone()),
        ("POST", "/feedback") if shared.state().refuse_feedback => Reply::status(500),
        ("RECORD" | "FLUSH" | "TEARDOWN" | "OPTIONS", _)
        | ("PUT", "/setProperty")
        | ("POST", "/feedback") => Reply::ok(),
        _ => Reply::status(404),
    }
}

/// AirPlay 1's ANNOUNCE: the stream, and the key its sound is encrypted
/// with if it is, wrapped with the fakes' key. One that takes sound only
/// encrypted refuses a stream without the key.
fn announce(shared: &Shared, request: &Received) -> Reply {
    let sdp = String::from_utf8_lossy(&request.body);
    let attribute = |name: &str| {
        sdp.lines().find_map(|line| {
            let value = line.strip_prefix("a=")?.strip_prefix(name)?;
            Some(value.strip_prefix(':')?.trim().to_owned())
        })
    };
    let block = |bytes: Vec<u8>| <[u8; crypto::BLOCK]>::try_from(bytes).ok();
    let key = attribute("rsaaeskey")
        .and_then(|text| crypto::unbase64(&text))
        .and_then(|wrapped| PrivateKey::fake().decrypt(&wrapped))
        .and_then(block);
    let iv = attribute("aesiv")
        .and_then(|text| crypto::unbase64(&text))
        .and_then(block);
    let announced = match (key, iv) {
        (Some(key), Some(iv)) => Encryption::Aes { key, iv },
        _ if shared.kind == Kind::EncryptedSpeaker => return Reply::status(400),
        _ => Encryption::None,
    };
    shared.state().announced = announced;
    Reply::ok()
}

/// AirPlay 1's SETUP: the sender's ports in the Transport header, ours in
/// the answer's.
fn setup_v1(shared: &Arc<Shared>, request: &Received) -> Reply {
    let transport = request.header("Transport").unwrap_or_default();
    let resends = transport
        .split(';')
        .filter_map(|part| part.split_once('='))
        .find(|(name, _)| *name == "control_port")
        .and_then(|(_, port)| port.parse::<u16>().ok());
    let announced = shared.state().announced;
    let (data, control) = take_sound(shared, announced, resends);
    let latency = shared.state().audio_latency;
    Reply::ok()
        .header(
            "Transport",
            format!(
                "RTP/AVP/UDP;unicast;mode=record;server_port={data};control_port={control};timing_port=0"
            ),
        )
        .header("Session", "1")
        .header("Audio-Latency", latency)
}

/// AirPlay 2's SETUP: first the session's, which has it ask the sender's
/// clock and gives an event connection; then the stream's, which gives
/// the ports sound goes to.
fn setup(shared: &Arc<Shared>, request: &Received, secret: Option<&[u8]>) -> Reply {
    let body = request.dictionary();
    let port = |dictionary: &Dictionary, key: &str| {
        dictionary
            .get(key)
            .and_then(Value::as_unsigned_integer)
            .and_then(|port| u16::try_from(port).ok())
    };
    if let Some(stream) = body
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| streams.first())
        .and_then(Value::as_dictionary)
    {
        let key = stream
            .get("shk")
            .and_then(Value::as_data)
            .and_then(|key| <[u8; 32]>::try_from(key).ok());
        let encryption = key.map_or(Encryption::None, Encryption::Sealed);
        let (data, control) = take_sound(shared, encryption, port(stream, "controlPort"));
        let mut answer = Dictionary::new();
        answer.insert("type".into(), int(0x60));
        answer.insert("dataPort".into(), int(data.into()));
        answer.insert("controlPort".into(), int(control.into()));
        let mut streams = Dictionary::new();
        streams.insert(
            "streams".into(),
            Value::Array(vec![Value::Dictionary(answer)]),
        );
        return Reply::ok().plist(streams);
    }
    if let Some(port) = port(&body, "timingPort") {
        ask_the_time(shared, SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port));
    }
    let mut answer = Dictionary::new();
    if let Some(secret) = secret {
        let port = talk_back(shared, Keys::events(secret).swapped());
        answer.insert("eventPort".into(), int(port.into()));
    }
    answer.insert("timingPort".into(), int(0));
    Reply::ok().plist(answer)
}

/// Ask the sender's clock the time, and keep the answer.
fn ask_the_time(shared: &Shared, to: SocketAddr) {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)) else {
        return;
    };
    let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
    let sent = rtp::ntp_now();
    if socket
        .send_to(&receiver::timing_request(1, sent), to)
        .is_err()
    {
        return;
    }
    let mut answer = [0; 64];
    if let Ok(n) = socket.recv(&mut answer) {
        shared.state().timing = Some((sent, answer[..n].to_vec()));
    }
}

/// Listen for the event connection; once the sender's on it, say something
/// and keep the status of its answer.
fn talk_back(shared: &Arc<Shared>, keys: Keys) -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port for events");
    let port = listener.local_addr().expect("the event port").port();
    let _ = listener.set_nonblocking(true);
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(_) if Instant::now() < deadline && !shared.gone() => thread::sleep(POLL),
                Err(_) => return,
            }
        };
        let _ = stream.set_nonblocking(false);
        shared.keep(&stream);
        let mut sealed = Sealed::new(stream, keys);
        let request = Message {
            start: "POST /command RTSP/1.0".into(),
            headers: vec![
                ("CSeq".into(), "1".into()),
                ("Content-Length".into(), "0".into()),
            ],
            body: Vec::new(),
        };
        if sealed.write_all(&request.encode()).is_err() {
            return;
        }
        if let Ok(Some(answer)) = Reader::default().read(&mut sealed) {
            shared.state().event_reply = Some(answer.status());
        }
        // Hold it open until one side goes.
        let _ = sealed.read_to_end(&mut Vec::new());
    });
    port
}

/// An audio packet's sequence number, RTP time and samples.
fn open(packet: &[u8], encryption: &Encryption) -> Option<(u16, u32, Vec<i16>)> {
    let (seq, timestamp) = receiver::audio(packet)?;
    let payload = match encryption {
        Encryption::Sealed(key) => {
            // The header, the sealed samples, and the nonce's eight bytes.
            let sealed = packet.len().checked_sub(12 + 8)?;
            let (sealed, nonce) = packet[12..].split_at(sealed);
            crypto::open(key, nonce.try_into().ok()?, &packet[4..12], sealed).ok()?
        }
        Encryption::Aes { key, iv } => {
            let mut payload = packet[12..].to_vec();
            crypto::cbc_decrypt(key, iv, &mut payload);
            payload
        }
        Encryption::None => packet[12..].to_vec(),
    };
    Some((seq, timestamp, receiver::samples(&payload)))
}

/// Take sound on a data port and a control port, which it gives; `resends`
/// is where the sender takes requests to send packets again.
fn take_sound(shared: &Arc<Shared>, encryption: Encryption, resends: Option<u16>) -> (u16, u16) {
    let bind = || UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port for sound");
    let data = bind();
    let control = Arc::new(bind());
    let ports = (
        data.local_addr().expect("the data port").port(),
        control.local_addr().expect("the control port").port(),
    );
    if let Some(port) = resends {
        shared.state().resends = Some((
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port),
            Arc::clone(&control),
        ));
    }
    let on_data = Arc::clone(shared);
    thread::spawn(move || {
        let _ = data.set_read_timeout(Some(POLL));
        let mut buffer = [0; 2048];
        while !on_data.gone() {
            if let Ok(n) = data.recv(&mut buffer)
                && let Some((seq, timestamp, samples)) = open(&buffer[..n], &encryption)
            {
                on_data
                    .state()
                    .packets
                    .insert(timestamp, Packet { seq, samples });
            }
        }
    });
    let on_control = Arc::clone(shared);
    thread::spawn(move || {
        let _ = control.set_read_timeout(Some(POLL));
        let mut buffer = [0; 2048];
        while !on_control.gone() {
            let Ok(n) = control.recv(&mut buffer) else {
                continue;
            };
            let packet = &buffer[..n];
            if let Some(sync) = receiver::sync(packet) {
                on_control.state().syncs.push(sync);
            } else if let Some(original) = receiver::resent(packet)
                && let Some((seq, _, _)) = open(original, &encryption)
            {
                on_control.state().resent.push(seq);
            }
        }
    });
    ports
}
