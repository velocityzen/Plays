use std::net::{Ipv4Addr, TcpListener};
use std::thread;

use super::*;

/// Reads at most `step` bytes at a time, as a slow network gives them.
pub(super) struct Trickle<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) step: usize,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.step).min(self.bytes.len());
        buf[..n].copy_from_slice(&self.bytes[..n]);
        self.bytes = &self.bytes[n..];
        Ok(n)
    }
}

pub(super) fn read(wire: &[u8], to_end: bool) -> io::Result<Option<Message>> {
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
