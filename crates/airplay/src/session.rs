//! What sound and video sessions share: the control connection, paired
//! and encrypted; with AirPlay 2, the session set up on it, the event
//! connection the receiver talks back on, and feedback every two seconds
//! that keeps it all alive. Also the answers to the receiver's clock
//! requests.

use std::io::Write;
use std::net::{Shutdown, SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use plist::{Dictionary, Value};

use crate::channel::{Keys, Sealed};
use crate::connection::{Connection, Request};
use crate::device::{Device, Pairing};
use crate::error::{Error, Result};
use crate::message::{Message, Reader};
use crate::pairing::Credentials;
use crate::rtp;

const FEEDBACK: Duration = Duration::from_secs(2);
/// How often threads waiting on the network look up to see if they are to
/// stop.
const POLL: Duration = Duration::from_millis(100);

/// Who is sending, as receivers show it.
#[derive(Clone, Debug)]
pub struct Sender {
    pub name: String,
}

impl Default for Sender {
    fn default() -> Self {
        Self {
            name: "AirPlay".into(),
        }
    }
}

/// What a session says as it goes, over its channel.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Event {
    /// The receiver went away, or ended the session.
    Gone,
}

/// A session's connection and the threads that keep it.
pub(crate) struct Control {
    connection: Arc<Mutex<Connection>>,
    /// The secret pairing gave, for the other connections' keys.
    pub secret: Option<Vec<u8>>,
    /// The RTSP session, once SETUP says it.
    pub rtsp_session: Option<String>,
    pub remote: SocketAddr,
    /// Where we answer the receiver's clock requests.
    pub timing_port: u16,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    /// To unblock the event thread.
    event_stream: Option<TcpStream>,
    pub events: UnboundedSender<Event>,
}

fn lock(connection: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    connection
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Control {
    /// Connect, and pair as `device` wants: with `credentials`, or
    /// transiently.
    pub fn connect(
        device: &Device,
        credentials: Option<&Credentials>,
    ) -> Result<(Self, UnboundedReceiver<Event>)> {
        let mut connection = Connection::open(&device.addresses)?;
        let secret = match (credentials, device.pairing()) {
            (Some(credentials), _) => Some(connection.verify(credentials)?),
            (None, Pairing::Pin) => return Err(Error::NeedsPin),
            (None, Pairing::Password) => {
                return Err(Error::pairing("it has a password, which isn't supported"));
            }
            (None, Pairing::Transient) => Some(connection.pair_transient()?),
            (None, Pairing::None) => None,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let timing = UdpSocket::bind((connection.local, 0))?;
        let timing_port = timing.local_addr()?.port();
        let (events, receiver) = unbounded();
        let control = Self {
            remote: connection.remote,
            connection: Arc::new(Mutex::new(connection)),
            secret,
            rtsp_session: None,
            timing_port,
            threads: vec![spawn(
                "airplay timing",
                answer_timing(timing, Arc::clone(&stop)),
            )?],
            stop,
            event_stream: None,
            events,
        };
        Ok((control, receiver))
    }

    pub fn connection(&self) -> MutexGuard<'_, Connection> {
        lock(&self.connection)
    }

    /// The session's URI, for its RTSP requests.
    pub fn uri(&self) -> String {
        self.connection().uri()
    }

    /// Send `request`, which is to succeed, with the session's header.
    pub fn exchange(&self, request: Request) -> Result<Message> {
        let request = match &self.rtsp_session {
            Some(session) => request.header("Session", session),
            None => request,
        };
        let response = self.connection().exchange(request)?;
        Ok(response)
    }

    /// AirPlay 2's first SETUP: who we are, and where our clock is. Then
    /// the event connection it gives, and feedback.
    pub fn setup(&mut self, sender: &Sender) -> Result<()> {
        let mut body = Dictionary::new();
        let mut put = |key: &str, value: Value| {
            body.insert(key.to_owned(), value);
        };
        put("deviceID", "AA:BB:CC:DD:EE:FF".into());
        put("macAddress", "AA:BB:CC:DD:EE:FF".into());
        put("sessionUUID", session_uuid().into());
        put(
            "timingPort",
            Value::Integer(u64::from(self.timing_port).into()),
        );
        put("timingProtocol", "NTP".into());
        put("isMultiSelectAirPlay", true.into());
        put("groupContainsGroupLeader", false.into());
        put("model", "iPhone14,3".into());
        put("name", sender.name.clone().into());
        put("osBuildVersion", "20F66".into());
        put("osName", "iPhone OS".into());
        put("osVersion", "16.5".into());
        put("senderSupportsRelay", false.into());
        put("sourceVersion", "690.7.1".into());
        put("statsCollectionEnabled", false.into());
        let uri = self.uri();
        let response = self.exchange(Request::rtsp("SETUP", uri).plist(Value::Dictionary(body)))?;
        if let Some(session) = response.header("Session") {
            self.rtsp_session = Some(session.to_owned());
        }
        let answer = response.dictionary()?;
        if let Some(port) = unsigned(answer.get("eventPort")).filter(|port| *port != 0) {
            self.listen_for_events(port as u16)?;
        }
        self.keep_alive()
    }

    /// Connect to where the receiver talks back, and answer it.
    fn listen_for_events(&mut self, port: u16) -> Result<()> {
        let address = SocketAddr::new(self.remote.ip(), port);
        // It may not be listening yet.
        let mut tries = 0;
        let stream = loop {
            match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
                Ok(stream) => break stream,
                Err(err) if tries < 4 => {
                    log::debug!("airplay: event connection: {err}; again");
                    tries += 1;
                    thread::sleep(Duration::from_millis(250));
                }
                Err(err) => return Err(err.into()),
            }
        };
        let keys = self
            .secret
            .as_deref()
            .map(Keys::events)
            .ok_or_else(|| Error::protocol("an event connection without pairing"))?;
        self.event_stream = Some(stream.try_clone()?);
        let stop = Arc::clone(&self.stop);
        let events = self.events.clone();
        self.threads.push(spawn("airplay events", move || {
            answer_events(Sealed::new(stream, keys), &stop, &events);
        })?);
        Ok(())
    }

    /// Send feedback every two seconds; it going unanswered means the
    /// receiver is gone.
    fn keep_alive(&mut self) -> Result<()> {
        let connection = Arc::clone(&self.connection);
        let stop = Arc::clone(&self.stop);
        let events = self.events.clone();
        self.threads.push(spawn("airplay feedback", move || {
            let mut last = Instant::now();
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(POLL);
                if last.elapsed() < FEEDBACK {
                    continue;
                }
                last = Instant::now();
                let sent = lock(&connection).exchange(Request::rtsp("POST", "/feedback"));
                if let Err(err) = sent {
                    if !stop.load(Ordering::Relaxed) {
                        log::info!("airplay: feedback went unanswered: {err}");
                        let _ = events.unbounded_send(Event::Gone);
                    }
                    break;
                }
            }
        })?);
        Ok(())
    }

    /// End the session: TEARDOWN, and the threads stopped.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let uri = self.uri();
        if let Err(err) = self.exchange(Request::rtsp("TEARDOWN", uri)) {
            log::debug!("airplay: teardown: {err}");
        }
        if let Some(stream) = self.event_stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let _ = self.connection().tcp().shutdown(Shutdown::Both);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }

    /// Run `work` on a thread of the session's, stopped with it.
    pub fn spawn(
        &mut self,
        name: &str,
        work: impl FnOnce(Arc<AtomicBool>) + Send + 'static,
    ) -> Result<()> {
        let stop = Arc::clone(&self.stop);
        self.threads.push(spawn(name, move || work(stop))?);
        Ok(())
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> Result<JoinHandle<()>> {
    Ok(thread::Builder::new().name(name.to_owned()).spawn(work)?)
}

/// A number from a property list.
pub(crate) fn unsigned(value: Option<&Value>) -> Option<u64> {
    value?.as_unsigned_integer()
}

/// A random UUID, upper case.
fn session_uuid() -> String {
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

/// Answer the receiver's clock requests until stopped.
fn answer_timing(socket: UdpSocket, stop: Arc<AtomicBool>) -> impl FnOnce() + Send {
    move || {
        let _ = socket.set_read_timeout(Some(POLL));
        let mut buffer = [0; 128];
        while !stop.load(Ordering::Relaxed) {
            let Ok((n, from)) = socket.recv_from(&mut buffer) else {
                continue;
            };
            if let Some(reply) = rtp::timing_reply(&buffer[..n], rtp::ntp_now()) {
                let _ = socket.send_to(&reply, from);
            }
        }
    }
}

/// Answer what the receiver sends on the event connection, until it
/// closes it or we stop.
fn answer_events(
    mut stream: Sealed<TcpStream>,
    stop: &AtomicBool,
    events: &UnboundedSender<Event>,
) {
    let mut reader = Reader::default();
    while let Ok(Some(request)) = reader.read(&mut stream) {
        log::debug!("airplay: the receiver says {}", request.start);
        let mut headers = vec![("Content-Length".to_owned(), "0".to_owned())];
        if let Some(cseq) = request.header("CSeq") {
            headers.push(("CSeq".to_owned(), cseq.to_owned()));
        }
        let reply = Message {
            start: "RTSP/1.0 200 OK".into(),
            headers,
            body: Vec::new(),
        };
        if stream.write_all(&reply.encode()).is_err() {
            break;
        }
    }
    if !stop.load(Ordering::Relaxed) {
        log::info!("airplay: the receiver closed the event connection");
        let _ = events.unbounded_send(Event::Gone);
    }
}
