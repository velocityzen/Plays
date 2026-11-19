//! The control connection to a receiver: RTSP and HTTP requests on one
//! TCP connection, encrypted once paired.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use crate::channel::{Keys, Sealed};
use crate::error::{Error, Result};
use crate::message::{Message, Reader};
use crate::pairing::{Credentials, Setup, Verify};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a receiver has to answer a request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = "AirPlay/550.10";

pub(crate) enum Link {
    Plain(TcpStream),
    Sealed(Sealed<TcpStream>),
}

impl Link {
    fn tcp(&self) -> &TcpStream {
        match self {
            Self::Plain(stream) => stream,
            Self::Sealed(sealed) => sealed.get_ref(),
        }
    }
}

impl Read for Link {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf),
            Self::Sealed(sealed) => sealed.read(buf),
        }
    }
}

impl Write for Link {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buf),
            Self::Sealed(sealed) => sealed.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Sealed(sealed) => sealed.flush(),
        }
    }
}

/// A request to send.
pub(crate) struct Request {
    method: String,
    uri: String,
    version: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    pub fn rtsp(method: &str, uri: impl Into<String>) -> Self {
        Self::new(method, uri, "RTSP/1.0")
    }

    pub fn http(method: &str, uri: impl Into<String>) -> Self {
        Self::new(method, uri, "HTTP/1.1")
    }

    fn new(method: &str, uri: impl Into<String>, version: &'static str) -> Self {
        Self {
            method: method.to_owned(),
            uri: uri.into(),
            version,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn header(mut self, name: &str, value: impl ToString) -> Self {
        self.headers.push((name.to_owned(), value.to_string()));
        self
    }

    pub fn body(self, content_type: &str, body: Vec<u8>) -> Self {
        let mut request = self.header("Content-Type", content_type);
        request.body = body;
        request
    }

    /// A binary property list.
    pub fn plist(self, value: plist::Value) -> Self {
        let mut body = Vec::new();
        plist::to_writer_binary(&mut body, &value).expect("a property list writes");
        self.body("application/x-apple-binary-plist", body)
    }

    /// `name: value` lines, as SET_PARAMETER takes.
    pub fn parameters(self, text: String) -> Self {
        self.body("text/parameters", text.into_bytes())
    }

    /// What it is, for errors.
    fn describe(&self) -> String {
        format!("{} {}", self.method, self.uri)
    }
}

/// A receiver's answer.
pub(crate) type Response = Message;

impl Message {
    pub fn status(&self) -> u16 {
        self.words().1.parse().unwrap_or(0)
    }

    /// The body, a property list dictionary.
    pub fn dictionary(&self) -> Result<plist::Dictionary> {
        plist::Value::from_reader(std::io::Cursor::new(&self.body))
            .ok()
            .and_then(plist::Value::into_dictionary)
            .ok_or_else(|| Error::protocol("an answer that isn't a property list"))
    }
}

pub(crate) struct Connection {
    link: Option<Link>,
    reader: Reader,
    cseq: u32,
    /// Our address on the network the receiver is on.
    pub local: IpAddr,
    pub remote: SocketAddr,
    /// The RTSP session's number.
    pub session: u32,
    /// Who we are to the receiver, should it talk back.
    dacp_id: String,
    active_remote: u32,
}

impl Connection {
    pub fn open(remote: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect_timeout(&remote, CONNECT_TIMEOUT)?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
        stream.set_nodelay(true)?;
        Ok(Self {
            local: stream.local_addr()?.ip(),
            link: Some(Link::Plain(stream)),
            reader: Reader::default(),
            cseq: 0,
            remote,
            session: rand::random(),
            dacp_id: format!("{:016X}", rand::random::<u64>()),
            active_remote: rand::random(),
        })
    }

    fn link(&mut self) -> &mut Link {
        self.link.as_mut().expect("a connection has its link")
    }

    pub fn tcp(&self) -> &TcpStream {
        self.link.as_ref().expect("a connection has its link").tcp()
    }

    /// Encrypt from now on.
    pub fn seal(&mut self, keys: Keys) {
        self.link = match self.link.take() {
            Some(Link::Plain(stream)) => Some(Link::Sealed(Sealed::new(stream, keys))),
            sealed => sealed,
        };
    }

    /// The session's RTSP URI.
    pub fn uri(&self) -> String {
        format!("rtsp://{}/{}", self.local, self.session)
    }

    /// Send `request`, and wait for its answer, whatever its status.
    pub fn send(&mut self, request: Request) -> Result<Response> {
        self.cseq += 1;
        let mut headers = vec![
            ("CSeq".to_owned(), self.cseq.to_string()),
            ("User-Agent".to_owned(), USER_AGENT.to_owned()),
            ("DACP-ID".to_owned(), self.dacp_id.clone()),
            ("Active-Remote".to_owned(), self.active_remote.to_string()),
            ("Client-Instance".to_owned(), self.dacp_id.clone()),
        ];
        headers.retain(|(name, _)| {
            !request
                .headers
                .iter()
                .any(|(theirs, _)| theirs.eq_ignore_ascii_case(name))
        });
        headers.extend(request.headers.iter().cloned());
        if !request.body.is_empty() {
            headers.push(("Content-Length".to_owned(), request.body.len().to_string()));
        }
        let message = Message {
            start: format!("{} {} {}", request.method, request.uri, request.version),
            headers,
            body: request.body.clone(),
        };
        log::trace!("airplay: {}", request.describe());
        self.link().write_all(&message.encode())?;
        let link = self.link.as_mut().expect("a connection has its link");
        let response = self
            .reader
            .read(link)?
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::UnexpectedEof))?;
        log::trace!("airplay: {} → {}", request.describe(), response.status());
        Ok(response)
    }

    /// Send `request`, which is to succeed.
    pub fn exchange(&mut self, request: Request) -> Result<Response> {
        let describe = request.describe();
        let response = self.send(request)?;
        match response.status() {
            200..=299 => Ok(response),
            status => Err(Error::Status {
                request: describe,
                status,
            }),
        }
    }

    fn pairing(&mut self, path: &str, hkp: u8, body: Vec<u8>) -> Result<Vec<u8>> {
        let request = Request::http("POST", path)
            .header("X-Apple-HKP", hkp)
            .body("application/octet-stream", body);
        Ok(self.exchange(request)?.body)
    }

    /// Transient pairing, then encrypt: for receivers open to everyone.
    pub fn pair_transient(&mut self) -> Result<Vec<u8>> {
        let mut setup = Setup::transient();
        let m2 = self.pairing("/pair-setup", 4, setup.m1())?;
        let m4 = self.pairing("/pair-setup", 4, setup.m3(&m2)?)?;
        let secret = setup.m4(&m4)?;
        self.seal(Keys::control(&secret));
        Ok(secret)
    }

    /// Pair-verify with what pairing with a PIN left, then encrypt.
    pub fn verify(&mut self, credentials: &Credentials) -> Result<Vec<u8>> {
        let mut verify = Verify::new(credentials);
        let m2 = self.pairing("/pair-verify", 3, verify.m1())?;
        let m4 = self.pairing("/pair-verify", 3, verify.m3(&m2)?)?;
        let secret = verify.m4(&m4)?.to_vec();
        self.seal(Keys::control(&secret));
        Ok(secret)
    }

    /// Have the receiver show a PIN to pair with.
    pub fn start_pin(&mut self) -> Result<()> {
        self.exchange(Request::http("POST", "/pair-pin-start"))?;
        Ok(())
    }

    /// Pair with the PIN the receiver shows.
    pub fn pair_pin(&mut self, pin: &str) -> Result<Credentials> {
        let mut setup = Setup::new(pin);
        let m2 = self.pairing("/pair-setup", 3, setup.m1())?;
        let m4 = self.pairing("/pair-setup", 3, setup.m3(&m2)?)?;
        setup.m4(&m4)?;
        let m6 = self.pairing("/pair-setup", 3, setup.m5()?)?;
        setup.m6(&m6)
    }
}
