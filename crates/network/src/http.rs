//! HTTP/1.1 and RTSP/1.0 messages as they come over a connection: a start
//! line, headers, and a body as long as `Content-Length` says, in chunks,
//! or to the end of the stream. What's taken is bounded, so a device
//! sending nonsense can't fill memory, and `Deadline` bounds how long a
//! message may take to come.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// The most of a head or a body taken: a description, an answer or an
/// event is a few kilobytes.
const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 1024 * 1024;
/// How much is read off a stream at a time.
const READ: usize = 8 * 1024;

/// A start line and headers.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Head {
    pub start: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// From its lines, which end with CRLF or LF alone.
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let start = lines.next()?.trim().to_owned();
        if start.is_empty() {
            return None;
        }
        let headers = lines
            .take_while(|line| !line.trim().is_empty())
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_owned(), value.trim().to_owned()))
            })
            .collect();
        Some(Self { start, headers })
    }

    /// A header by its name, whatever its case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// A response's status.
    pub fn status(&self) -> Option<u16> {
        let mut words = self.start.split_whitespace();
        words
            .next()
            .filter(|version| version.starts_with("HTTP/"))?;
        words.next()?.parse().ok()
    }

    /// A request's method and path.
    pub fn request(&self) -> Option<(&str, &str)> {
        let mut words = self.start.split_whitespace();
        let method = words.next()?;
        let path = words.next()?;
        words
            .next()
            .filter(|version| version.starts_with("HTTP/"))?;
        Some((method, path))
    }
}

/// A head and its body.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Message {
    pub head: Head,
    pub body: Vec<u8>,
}

impl Message {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.head.header(name)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The one message on a stream that carries no other, as a connection
    /// closed after it does; the stream ending first is an error. Its body
    /// runs to the end when it doesn't say how long it is and `to_end`, as
    /// a response's does; otherwise it's then empty, as a request's is.
    pub fn read(stream: impl Read, to_end: bool) -> io::Result<Self> {
        Reader::default()
            .read(stream, to_end)?
            .ok_or_else(|| closed("before a message"))
    }
}

/// Reads messages off a stream, one after another, keeping what it read
/// past one for the next.
#[derive(Default)]
pub struct Reader {
    buffer: Vec<u8>,
    /// Where what's yet to be taken from `buffer` starts.
    at: usize,
}

impl Reader {
    /// The next message; `None` once the stream ends between messages. Its
    /// body runs to the end of the stream when it doesn't say how long it
    /// is and `to_end`; otherwise it's then empty, as on a connection that
    /// stays open for the next message.
    pub fn read(&mut self, mut stream: impl Read, to_end: bool) -> io::Result<Option<Message>> {
        let Some(head) = self.head(&mut stream)? else {
            return Ok(None);
        };
        let chunked = head
            .header("Transfer-Encoding")
            .is_some_and(|coding| coding.to_ascii_lowercase().contains("chunked"));
        let body = if chunked {
            self.chunks(&mut stream)?
        } else if let Some(length) = head.header("Content-Length") {
            let length: usize = length
                .parse()
                .map_err(|_| invalid("a Content-Length that isn't a number"))?;
            if length > MAX_BODY {
                return Err(invalid("a body too long"));
            }
            self.take(&mut stream, length)?
        } else if to_end {
            self.rest(&mut stream)?
        } else {
            Vec::new()
        };
        Ok(Some(Message { head, body }))
    }

    /// The head, to the blank line after its headers; blank lines before
    /// its start line are skipped. `None` if the stream ends before it.
    fn head(&mut self, stream: &mut impl Read) -> io::Result<Option<Head>> {
        let mut head = Vec::new();
        // The blank lines skipped count too, so they can't go on forever.
        let mut taken = 0;
        loop {
            let Some(line) = self.line(stream, MAX_HEAD - taken)? else {
                return if head.is_empty() {
                    Ok(None)
                } else {
                    Err(closed("in the head"))
                };
            };
            taken += line.len();
            let blank = is_blank(&line);
            if blank && head.is_empty() {
                continue;
            }
            head.extend_from_slice(&line);
            if blank {
                break;
            }
        }
        Head::parse(&String::from_utf8_lossy(&head))
            .map(Some)
            .ok_or_else(|| invalid("no start line"))
    }

    /// A chunked body, and the trailer after its last chunk.
    fn chunks(&mut self, stream: &mut impl Read) -> io::Result<Vec<u8>> {
        let mut body = Vec::new();
        loop {
            let line = self
                .line(stream, MAX_HEAD)?
                .ok_or_else(|| closed("in the body"))?;
            let line = String::from_utf8_lossy(&line);
            let size = line.trim().split(';').next().unwrap_or_default();
            let size =
                usize::from_str_radix(size.trim(), 16).map_err(|_| invalid("a bad chunk size"))?;
            if size == 0 {
                // The trailer, to its blank line or the end of the stream.
                let mut taken = 0;
                while let Some(line) = self.line(stream, MAX_HEAD - taken)? {
                    taken += line.len();
                    if is_blank(&line) {
                        break;
                    }
                }
                return Ok(body);
            }
            // Not `body.len() + size`, which a chunk size could overflow.
            if size > MAX_BODY - body.len() {
                return Err(invalid("a body too long"));
            }
            body.extend(self.take(stream, size)?);
            // The line break after the chunk.
            self.line(stream, MAX_HEAD)?;
        }
    }

    /// The next line, with its line break; `None` if the stream ends
    /// before any of it. One longer than `limit` is an error.
    fn line(&mut self, stream: &mut impl Read, limit: usize) -> io::Result<Option<Vec<u8>>> {
        // How much of what's pending has no line break, so isn't searched
        // again.
        let mut searched = 0;
        loop {
            let pending = &self.buffer[self.at..];
            if let Some(end) = pending[searched..].iter().position(|&byte| byte == b'\n') {
                let end = searched + end + 1;
                if end > limit {
                    return Err(invalid("a line too long"));
                }
                let line = pending[..end].to_vec();
                self.at += end;
                return Ok(Some(line));
            }
            searched = pending.len();
            if searched > limit {
                return Err(invalid("a line too long"));
            }
            if !self.fill(stream)? {
                return if self.buffer.len() == self.at {
                    Ok(None)
                } else {
                    Err(closed("in a line"))
                };
            }
        }
    }

    /// The next `n` bytes; the stream ending first is an error.
    fn take(&mut self, stream: &mut impl Read, n: usize) -> io::Result<Vec<u8>> {
        while self.buffer.len() - self.at < n {
            if !self.fill(stream)? {
                return Err(closed("in the body"));
            }
        }
        let taken = self.buffer[self.at..self.at + n].to_vec();
        self.at += n;
        Ok(taken)
    }

    /// The rest of the stream, which may be no longer than a body.
    fn rest(&mut self, stream: &mut impl Read) -> io::Result<Vec<u8>> {
        loop {
            if self.buffer.len() - self.at > MAX_BODY {
                return Err(invalid("a body too long"));
            }
            if !self.fill(stream)? {
                break;
            }
        }
        let rest = self.buffer.split_off(self.at);
        self.buffer.clear();
        self.at = 0;
        Ok(rest)
    }

    /// Read more after what's pending, letting go of what's taken; false
    /// at the end of the stream.
    fn fill(&mut self, stream: &mut impl Read) -> io::Result<bool> {
        self.buffer.drain(..self.at);
        self.at = 0;
        let mut chunk = [0; READ];
        let n = loop {
            match stream.read(&mut chunk) {
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                read => break read?,
            }
        };
        self.buffer.extend_from_slice(&chunk[..n]);
        Ok(n > 0)
    }
}

fn is_blank(line: &[u8]) -> bool {
    line.iter().all(u8::is_ascii_whitespace)
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

fn closed(within: &str) -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, format!("closed {within}"))
}

/// What can be told how long a read may wait: a TCP stream, or a borrow
/// of one.
pub trait ReadTimeout {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
}

impl ReadTimeout for TcpStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }
}

impl<T: ReadTimeout + ?Sized> ReadTimeout for &T {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        (**self).set_read_timeout(timeout)
    }
}

impl<T: ReadTimeout + ?Sized> ReadTimeout for &mut T {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        (**self).set_read_timeout(timeout)
    }
}

/// Reads from a stream until a deadline, each read waiting only as long
/// as is left: a device sending a byte at a time takes no longer than a
/// whole message may. Writes go straight through.
pub struct Deadline<S> {
    stream: S,
    /// None while there's none: reads then wait as long as the stream's
    /// own timeout says.
    until: Option<Instant>,
}

impl<S> Deadline<S> {
    /// Reads from `stream` that are to be done `within` from now.
    pub fn new(stream: S, within: Duration) -> Self {
        Self {
            stream,
            until: Instant::now().checked_add(within),
        }
    }

    /// `stream`, with no deadline until one is started.
    pub fn unset(stream: S) -> Self {
        Self {
            stream,
            until: None,
        }
    }

    /// Reads from now on are to be done `within` from now: for the next
    /// message on a connection that carries many.
    pub fn start(&mut self, within: Duration) {
        self.until = Instant::now().checked_add(within);
    }

    pub fn get_ref(&self) -> &S {
        &self.stream
    }
}

impl<S: Read + ReadTimeout> Read for Deadline<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(until) = self.until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "no whole message in time",
                ));
            }
            self.stream.set_read_timeout(Some(left))?;
        }
        self.stream.read(buf)
    }
}

impl<S: Write> Write for Deadline<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[cfg(test)]
#[path = "tests/http_properties.rs"]
mod properties;
#[cfg(test)]
#[path = "tests/http.rs"]
mod tests;
