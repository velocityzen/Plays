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
mod tests {
    use std::net::{Ipv4Addr, TcpListener};
    use std::thread;

    use super::*;

    /// Reads at most `step` bytes at a time, as a slow network gives them.
    struct Trickle<'a> {
        bytes: &'a [u8],
        step: usize,
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.step).min(self.bytes.len());
            buf[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }

    fn read(wire: &[u8], to_end: bool) -> io::Result<Option<Message>> {
        Reader::default().read(wire, to_end)
    }

    #[test]
    fn heads_have_headers_whatever_their_case() {
        let head = Head::parse(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age = 1800\r\nLocation:http://a/b\r\n\r\n",
        )
        .unwrap();
        assert_eq!(head.status(), Some(200));
        assert_eq!(head.header("cache-control"), Some("max-age = 1800"));
        assert_eq!(head.header("LOCATION"), Some("http://a/b"));
        let search = Head::parse("M-SEARCH * HTTP/1.1\nST: ssdp:all\n").unwrap();
        assert_eq!(search.request(), Some(("M-SEARCH", "*")));
        assert_eq!(search.status(), None);
    }

    #[test]
    fn messages_one_after_another_keep_what_came_past_each() {
        let wire = b"RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: 5\r\n\r\nhello\
                     POST /feedback RTSP/1.0\r\nCSeq: 2\r\n\r\n";
        for step in [1, 7, wire.len()] {
            let mut stream = Trickle { bytes: wire, step };
            let mut reader = Reader::default();
            let first = reader.read(&mut stream, false).unwrap().unwrap();
            assert_eq!(first.head.start, "RTSP/1.0 200 OK");
            assert_eq!(first.header("cseq"), Some("1"));
            assert_eq!(first.body, b"hello");
            let second = reader.read(&mut stream, false).unwrap().unwrap();
            assert_eq!(second.head.start, "POST /feedback RTSP/1.0");
            assert_eq!(second.header("CSeq"), Some("2"));
            assert!(second.body.is_empty());
            assert!(reader.read(&mut stream, false).unwrap().is_none());
        }
    }

    #[test]
    fn lines_may_end_in_lf_alone_and_blank_ones_before_a_message_are_skipped() {
        let message = read(
            b"\r\n\n  \r\nHTTP/1.1 200 OK\nContent-Length: 2\n\nhi",
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(message.head.status(), Some(200));
        assert_eq!(message.header("Content-Length"), Some("2"));
        assert_eq!(message.body, b"hi");
        assert!(read(b"\r\n\r\n", false).unwrap().is_none(), "no message");
    }

    #[test]
    fn a_chunked_body_is_put_together() {
        let wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                     5;x=y\r\nhello\r\n6\r\n world\r\n0\r\nExpires: never\r\n\r\nnext";
        let mut stream = &wire[..];
        let mut reader = Reader::default();
        let message = reader.read(&mut stream, false).unwrap().unwrap();
        assert_eq!(message.text(), "hello world");
        assert!(
            reader.read(&mut stream, false).is_err(),
            "the trailer's taken, and what's after is no message"
        );
    }

    #[test]
    fn a_body_without_a_length_runs_to_the_end_only_when_asked() {
        let wire = b"HTTP/1.0 200 OK\r\n\r\nuntil it closes";
        assert_eq!(read(wire, true).unwrap().unwrap().text(), "until it closes");
        let mut stream = &wire[..];
        let mut reader = Reader::default();
        assert!(
            reader
                .read(&mut stream, false)
                .unwrap()
                .unwrap()
                .body
                .is_empty()
        );
        assert!(
            reader.read(&mut stream, false).is_err(),
            "what follows is no message"
        );
    }

    #[test]
    fn a_stream_ending_partway_is_an_error_and_between_messages_is_none() {
        assert!(read(b"", false).unwrap().is_none());
        assert!(read(b"HTTP/1.1 200 OK\r\nServer: x", false).is_err());
        assert!(read(b"HTTP/1.1 200 OK\r\nServer: x\r\n", false).is_err());
        assert!(read(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort", false).is_err());
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhel";
        assert!(read(chunked, false).is_err());
        assert_eq!(
            Message::read(&b""[..], true).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof,
            "one message is to come"
        );
    }

    #[test]
    fn a_length_that_isnt_a_number_or_is_past_the_most_is_an_error() {
        for length in ["18446744073709551615", "18446744073709551616", "-1", "ten"] {
            let wire = format!("RTSP/1.0 200 OK\r\nContent-Length: {length}\r\n\r\n");
            assert!(read(wire.as_bytes(), false).is_err(), "{length}");
        }
        let at_most = format!("HTTP/1.1 200 OK\r\nContent-Length: {MAX_BODY}\r\n\r\n");
        let mut wire = at_most.into_bytes();
        wire.resize(wire.len() + MAX_BODY, b'x');
        assert_eq!(read(&wire, false).unwrap().unwrap().body.len(), MAX_BODY);
        let over = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        let mut wire = over.into_bytes();
        wire.resize(wire.len() + MAX_BODY + 1, b'x');
        assert!(read(&wire, false).is_err(), "a byte past the most");
    }

    #[test]
    fn a_body_to_the_end_past_the_most_is_an_error_not_cut_short() {
        let mut wire = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        wire.resize(wire.len() + MAX_BODY, b'x');
        assert_eq!(read(&wire, true).unwrap().unwrap().body.len(), MAX_BODY);
        wire.push(b'x');
        assert!(read(&wire, true).is_err());
    }

    #[test]
    fn chunks_too_long_are_errors() {
        let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        let huge = format!("{head}5\r\nhello\r\nfffffffffffffffc\r\n");
        assert!(read(huge.as_bytes(), false).is_err(), "past what counts");
        let mut over = format!("{head}{:x}\r\n", MAX_BODY + 1).into_bytes();
        over.extend([0; 64]);
        assert!(
            read(&over, false).is_err(),
            "a byte past the most a body may be"
        );
        let mut two = format!("{head}{MAX_BODY:x}\r\n").into_bytes();
        two.resize(two.len() + MAX_BODY, b'x');
        two.extend(b"\r\n1\r\nx\r\n0\r\n\r\n");
        assert!(read(&two, false).is_err(), "together past the most");
        assert!(read(format!("{head}zz\r\n").as_bytes(), false).is_err());
    }

    #[test]
    fn a_head_past_the_most_is_an_error() {
        let mut long = b"HTTP/1.1 200 OK\r\nX-Long: ".to_vec();
        long.resize(MAX_HEAD + 1, b'x');
        long.extend(b"\r\n\r\n");
        assert!(read(&long, false).is_err(), "one line too long");
        let mut many = b"HTTP/1.1 200 OK\r\n".to_vec();
        while many.len() <= MAX_HEAD {
            many.extend(b"X-Short: x\r\n");
        }
        many.extend(b"\r\n");
        assert!(read(&many, false).is_err(), "too many lines");
        let mut blank = vec![b'\n'; MAX_HEAD + 1];
        blank.extend(b"HTTP/1.1 200 OK\r\n\r\n");
        assert!(read(&blank, false).is_err(), "too many blank lines first");
    }

    #[test]
    fn a_message_sent_a_byte_at_a_time_takes_no_longer_than_its_deadline() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n");
            // Each byte well within the wait, but never the whole head.
            for _ in 0..200 {
                thread::sleep(Duration::from_millis(50));
                if stream.write_all(b"X").is_err() {
                    return;
                }
            }
        });
        let stream = TcpStream::connect(address).unwrap();
        let started = Instant::now();
        let read = Message::read(Deadline::new(&stream, Duration::from_millis(500)), true);
        let took = started.elapsed();
        assert!(read.is_err(), "no whole message came");
        assert!(took < Duration::from_secs(2), "it took {took:?}");
    }

    #[test]
    fn a_deadline_started_again_counts_from_then() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"RTSP/1.0 200 OK\r\n\r\n").unwrap();
            thread::sleep(Duration::from_millis(300));
            stream.write_all(b"RTSP/1.0 200 OK\r\n\r\n").unwrap();
            stream
        });
        let mut stream = Deadline::unset(TcpStream::connect(address).unwrap());
        let mut reader = Reader::default();
        assert!(reader.read(&mut stream, false).unwrap().is_some());
        stream.start(Duration::from_millis(100));
        let late = reader.read(&mut stream, false).unwrap_err().kind();
        assert!(
            matches!(late, io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut),
            "{late:?}"
        );
        let _server = server.join().unwrap();
        stream.start(Duration::from_secs(2));
        assert!(
            Reader::default()
                .read(&mut stream, false)
                .unwrap()
                .is_some()
        );
    }
}
