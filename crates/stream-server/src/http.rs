//! A small HTTP/1.1 server: a thread for each connection, kept open between
//! requests, with GET, HEAD and OPTIONS; byte ranges of whatever has a
//! known length; and the CORS headers Chromecast's receiver needs.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use network::Accepted;

/// How long a kept-open connection waits for its next request, head and
/// all: one sent a byte at a time takes no longer.
#[cfg(not(test))]
const IDLE: Duration = Duration::from_secs(30);
#[cfg(test)]
const IDLE: Duration = Duration::from_secs(1);
/// How many connections are served at once; more are closed as they come.
#[cfg(not(test))]
pub(crate) const MAX_CONNECTIONS: usize = 64;
#[cfg(test)]
pub(crate) const MAX_CONNECTIONS: usize = 4;
/// How long to wait after taking a connection failed, as it does while
/// the app is out of file descriptors, before trying again.
const ACCEPT_AGAIN: Duration = Duration::from_millis(100);
/// How long a write may stall, as a paused TV's does, before giving up.
const STALL: Duration = Duration::from_secs(300);
/// The most a request's line and headers may take.
const MAX_HEAD: usize = 16 * 1024;

pub(crate) const SERVER: &str = "stream-server/0.1 UPnP/1.0 DLNADOC/1.50";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Method {
    Get,
    Head,
    Options,
    Other,
}

#[derive(Debug)]
pub(crate) struct Request {
    pub method: Method,
    pub path: String,
    pub query: String,
    /// Names in lower case.
    pub headers: Vec<(String, String)>,
    /// Who asked, when it came over the network.
    pub peer: Option<IpAddr>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// A query parameter's value.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }
}

pub(crate) enum Body {
    Empty,
    Bytes(Arc<Vec<u8>>),
    File {
        path: PathBuf,
        len: u64,
    },
    /// Of no known length, sent until the sender stops; the connection
    /// closes after it.
    Stream(Receiver<Vec<u8>>),
}

impl Body {
    fn len(&self) -> Option<u64> {
        match self {
            Self::Empty => Some(0),
            Self::Bytes(bytes) => Some(bytes.len() as u64),
            Self::File { len, .. } => Some(*len),
            Self::Stream(_) => None,
        }
    }
}

pub(crate) struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Response {
    pub fn new(status: u16, body: Body) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }

    pub fn bytes(content_type: &str, bytes: impl Into<Arc<Vec<u8>>>) -> Self {
        Self::new(200, Body::Bytes(bytes.into())).with("Content-Type", content_type)
    }

    pub fn text(content_type: &str, text: String) -> Self {
        Self::bytes(content_type, Arc::new(text.into_bytes()))
    }

    pub fn status(status: u16) -> Self {
        Self::new(status, Body::Empty)
    }

    pub fn with(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    fn has(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case(name))
    }
}

pub(crate) type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;

/// Accept connections from the peers `admit` lets in until `stop` is set
/// (and a connection wakes the listener to see it). Others are closed as
/// they come, before they take one of the places or a thread. Each is
/// `Accepted` as it's taken, so it closes for the device however it's let
/// go of.
pub(crate) fn serve(
    listener: TcpListener,
    handler: Handler,
    admit: impl Fn(IpAddr) -> bool,
    stop: Arc<AtomicBool>,
) {
    let open = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let stream = stream.map(Accepted::from);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let stream = match stream {
            Ok(stream) => stream,
            Err(err) => {
                log::debug!("can't take a connection: {err}");
                thread::sleep(ACCEPT_AGAIN);
                continue;
            }
        };
        // As an address of its own version, where IPv4's comes to IPv6's
        // listener.
        match stream.peer_addr() {
            Ok(peer) if admit(peer.ip().to_canonical()) => {}
            Ok(peer) => {
                log::debug!("{} isn't let in; closing", peer.ip());
                continue;
            }
            Err(err) => {
                log::debug!("can't take a connection: {err}");
                continue;
            }
        }
        let Some(slot) = Slot::take(&open) else {
            log::debug!("{MAX_CONNECTIONS} connections open; closing another");
            continue;
        };
        let handler = Arc::clone(&handler);
        let spawned = thread::Builder::new()
            .name("stream-server connection".into())
            .spawn(move || {
                let _slot = slot;
                if let Err(err) = connection(stream, &handler) {
                    log::debug!("a connection ended: {err}");
                }
            });
        if let Err(err) = spawned {
            log::warn!("can't take a connection: {err}");
        }
    }
}

/// One of the `MAX_CONNECTIONS` served at once, given back when dropped.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(open: &Arc<AtomicUsize>) -> Option<Self> {
        open.try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < MAX_CONNECTIONS).then_some(n + 1)
        })
        .ok()?;
        Some(Self(Arc::clone(open)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A connection's reading half, each read waiting only as long as is left
/// until `until`.
struct Timed {
    stream: TcpStream,
    until: Instant,
}

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "no whole request in time",
            ));
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

fn connection(stream: Accepted, handler: &Handler) -> io::Result<()> {
    stream.set_write_timeout(Some(STALL))?;
    let _ = stream.set_nodelay(true);
    let mut reader = BufReader::new(Timed {
        stream: stream.try_clone()?,
        until: Instant::now(),
    });
    let peer = stream.peer_addr()?.ip().to_canonical();
    let mut writer = stream;
    loop {
        reader.get_mut().until = Instant::now() + IDLE;
        let Some((mut request, keep_alive)) = read_request(&mut reader)? else {
            return Ok(());
        };
        request.peer = Some(peer);
        let response = if request.method == Method::Options {
            Response::status(204)
                .with("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS")
                .with(
                    "Access-Control-Allow-Headers",
                    request
                        .header("access-control-request-headers")
                        .unwrap_or("Range, Content-Type"),
                )
                .with("Access-Control-Max-Age", "86400")
        } else if request.method == Method::Other {
            Response::status(405).with("Allow", "GET, HEAD, OPTIONS")
        } else {
            handler(&request)
        };
        let keep_alive = write_response(&mut writer, &request, response, keep_alive)?;
        if !keep_alive {
            return Ok(());
        }
    }
}

/// The next line of a request's head into `line`, reading no further than
/// what `size` leaves of `MAX_HEAD`; how long it was, 0 at the end.
fn read_head_line(
    reader: &mut impl BufRead,
    line: &mut String,
    size: &mut usize,
) -> io::Result<usize> {
    let left = MAX_HEAD - *size;
    let mut bytes = Vec::new();
    // A byte past what's left, to tell a line that fits from one that doesn't.
    let n = reader
        .by_ref()
        .take(left as u64 + 1)
        .read_until(b'\n', &mut bytes)?;
    if n > left {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a request's head too long",
        ));
    }
    *size += n;
    *line = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "a head that isn't text"))?;
    Ok(n)
}

/// The next request, and whether the connection may stay open after it;
/// `None` when the client closed the connection.
fn read_request(reader: &mut impl BufRead) -> io::Result<Option<(Request, bool)>> {
    let mut line = String::new();
    // What of `MAX_HEAD` is used, blank lines before the request too.
    let mut size = 0;
    // Blank lines between requests are allowed.
    loop {
        match read_head_line(reader, &mut line, &mut size) {
            Ok(0) => return Ok(None),
            Ok(_) if line.trim().is_empty() => continue,
            Ok(_) => break,
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None);
            }
            Err(err) => return Err(err),
        }
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target), version) = (parts.next(), parts.next(), parts.next()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a malformed request",
        ));
    };
    let method = match method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => Method::Other,
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = Vec::new();
    let mut header = String::new();
    loop {
        if read_head_line(reader, &mut header, &mut size)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let request = Request {
        method,
        path: path.to_owned(),
        query: query.to_owned(),
        headers,
        peer: None,
    };
    // Whatever body came with it isn't wanted, but is read past.
    if let Some(len) = request
        .header("content-length")
        .and_then(|len| len.parse::<u64>().ok())
    {
        io::copy(&mut reader.by_ref().take(len), &mut io::sink())?;
    }
    let connection = request.header("connection").map(str::to_ascii_lowercase);
    let keep_alive = match version {
        Some("HTTP/1.0") => connection.as_deref() == Some("keep-alive"),
        _ => connection.as_deref() != Some("close"),
    };
    Ok(Some((request, keep_alive)))
}

/// A byte range a request asks for, of something `len` long: `Ok(None)`
/// for all of it, `Err` when it can't be had.
pub(crate) fn range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = header.and_then(|header| header.trim().strip_prefix("bytes=")) else {
        return Ok(None);
    };
    // Several ranges at once are answered with all of it, as HTTP allows.
    if spec.contains(',') {
        return Ok(None);
    }
    let (first, last) = spec.split_once('-').ok_or(())?;
    let (first, last) = (first.trim(), last.trim());
    let (start, end) = if first.is_empty() {
        let suffix: u64 = last.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        (len.saturating_sub(suffix), len.checked_sub(1).ok_or(())?)
    } else {
        let start: u64 = first.parse().map_err(|_| ())?;
        let end = if last.is_empty() {
            len.checked_sub(1).ok_or(())?
        } else {
            last.parse::<u64>()
                .map_err(|_| ())?
                .min(len.checked_sub(1).ok_or(())?)
        };
        (start, end)
    };
    if start >= len || start > end {
        return Err(());
    }
    Ok(Some((start, end)))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// Send the response, taking the range the request asks for; returns
/// whether the connection stays open.
fn write_response(
    writer: &mut TcpStream,
    request: &Request,
    mut response: Response,
    keep_alive: bool,
) -> io::Result<bool> {
    let len = response.body.len();
    let mut slice = None;
    if response.status == 200
        && let Some(len) = len
    {
        match range(request.header("range"), len) {
            Ok(Some((start, end))) => {
                response.status = 206;
                response
                    .headers
                    .push(("Content-Range".into(), format!("bytes {start}-{end}/{len}")));
                slice = Some((start, end - start + 1));
            }
            Ok(None) => {}
            Err(()) => {
                response = Response::status(416).with("Content-Range", format!("bytes */{len}"));
            }
        }
    }
    let streaming = matches!(response.body, Body::Stream(_));
    let keep_alive = keep_alive && !streaming;
    let len = response.body.len();
    let sent = slice.map_or(len, |(_, count)| Some(count));

    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    );
    let mut header = |name: &str, value: &str| {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    };
    header("Server", SERVER);
    header("Access-Control-Allow-Origin", "*");
    header(
        "Access-Control-Expose-Headers",
        "Content-Length, Content-Range, Accept-Ranges",
    );
    if let Some(sent) = sent {
        header("Content-Length", &sent.to_string());
        if response.status == 200 || response.status == 206 {
            header("Accept-Ranges", "bytes");
        }
    }
    header(
        "Connection",
        if keep_alive { "keep-alive" } else { "close" },
    );
    for (name, value) in &response.headers {
        header(name, value);
    }
    if !response.has("Content-Type") && len.is_some_and(|len| len > 0) {
        header("Content-Type", "application/octet-stream");
    }
    head.push_str("\r\n");
    writer.write_all(head.as_bytes())?;
    if request.method == Method::Head {
        writer.flush()?;
        return Ok(keep_alive);
    }

    let (offset, count) = slice.unwrap_or((0, len.unwrap_or(0)));
    match response.body {
        Body::Empty => {}
        Body::Bytes(bytes) => {
            writer.write_all(&bytes[offset as usize..(offset + count) as usize])?;
        }
        Body::File { path, .. } => {
            let mut file = File::open(path)?;
            file.seek(SeekFrom::Start(offset))?;
            let copied = io::copy(&mut file.take(count), writer)?;
            if copied < count {
                // The file got shorter; the length sent was a lie.
                return Ok(false);
            }
        }
        Body::Stream(chunks) => {
            for chunk in chunks {
                writer.write_all(&chunk)?;
            }
        }
    }
    writer.flush()?;
    Ok(keep_alive)
}

#[cfg(test)]
#[path = "tests/http_properties.rs"]
mod properties;
#[cfg(test)]
#[path = "tests/http.rs"]
mod tests;
