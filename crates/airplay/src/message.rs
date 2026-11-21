//! HTTP and RTSP messages, which AirPlay mixes on one connection: a start
//! line, headers, and a body as long as `Content-Length` says. They're
//! read with `network`'s reader, which bounds them.

use std::io::{self, Read};

use network::http;

#[derive(Clone, PartialEq, Debug)]
pub struct Message {
    /// The request line, or the status line.
    pub start: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Message {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The start line's words: method, URI and version; or version, status
    /// and reason.
    pub fn words(&self) -> (&str, &str, &str) {
        let mut words = self.start.splitn(3, ' ');
        (
            words.next().unwrap_or_default(),
            words.next().unwrap_or_default(),
            words.next().unwrap_or_default(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut head = format!("{}\r\n", self.start);
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if !self.body.is_empty() && self.header("Content-Length").is_none() {
            head.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

impl From<http::Message> for Message {
    fn from(message: http::Message) -> Self {
        Self {
            start: message.head.start,
            headers: message.head.headers,
            body: message.body,
        }
    }
}

/// Reads messages off a connection, keeping what it read past one for the
/// next: the connection stays open after each, so a body that doesn't say
/// how long it is is empty.
#[derive(Default)]
pub struct Reader(http::Reader);

impl Reader {
    /// The next message; `None` once the stream ends between messages.
    pub fn read(&mut self, stream: impl Read) -> io::Result<Option<Message>> {
        Ok(self.0.read(stream, false)?.map(Message::from))
    }
}

#[cfg(test)]
mod tests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn reads_messages_one_after_another_with_their_bodies() {
        let first = Message {
            start: "RTSP/1.0 200 OK".into(),
            headers: vec![("CSeq".into(), "1".into())],
            body: b"hello".to_vec(),
        };
        let second = Message {
            start: "POST /feedback RTSP/1.0".into(),
            headers: vec![("CSeq".into(), "2".into())],
            body: Vec::new(),
        };
        let mut wire = first.encode();
        wire.extend(second.encode());
        let mut stream = io::Cursor::new(wire);
        let mut reader = Reader::default();
        let read = reader.read(&mut stream).unwrap().unwrap();
        assert_eq!(read.body, b"hello");
        assert_eq!(read.header("content-length"), Some("5"));
        assert_eq!(read.words(), ("RTSP/1.0", "200", "OK"));
        let read = reader.read(&mut stream).unwrap().unwrap();
        assert_eq!(read, second);
        assert_eq!(reader.read(&mut stream).unwrap(), None);
    }

    #[test]
    fn a_message_cut_short_is_an_error() {
        let mut stream =
            io::Cursor::new(b"RTSP/1.0 200 OK\r\nContent-Length: 9\r\n\r\nshort".to_vec());
        assert!(Reader::default().read(&mut stream).is_err());
    }

    /// Reads at most `step` bytes at a time.
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

    /// A message with no `Content-Length`, which `encode` adds for a body.
    fn message() -> impl Strategy<Value = Message> {
        let header = (
            "[!-9;-~]{1,12}".prop_filter("the length", |name: &String| {
                !name.eq_ignore_ascii_case("Content-Length")
                    && !name.eq_ignore_ascii_case("Transfer-Encoding")
            }),
            "([!-~]{1,8}( [!-~]{1,8}){0,2})?",
        );
        (
            "[!-~]{1,10}( [!-~]{1,10}){0,2}",
            vec(header, 0..6),
            vec(any::<u8>(), 0..300),
        )
            .prop_map(|(start, headers, body)| Message {
                start,
                headers,
                body,
            })
    }

    proptest! {
        #[test]
        fn messages_read_back_one_after_another_however_they_come(
            messages in vec(message(), 1..4),
            step in 1usize..64,
        ) {
            let wire: Vec<u8> = messages.iter().flat_map(Message::encode).collect();
            let mut stream = Trickle { bytes: &wire, step };
            let mut reader = Reader::default();
            for mut message in messages {
                if !message.body.is_empty() {
                    message.headers.push(("Content-Length".into(), message.body.len().to_string()));
                }
                prop_assert_eq!(reader.read(&mut stream).unwrap(), Some(message));
            }
            prop_assert_eq!(reader.read(&mut stream).unwrap(), None);
        }

        #[test]
        fn a_content_length_past_a_mebibyte_is_refused_before_it_is_read(
            length in prop_oneof![(1u128 << 20) + 1..=2 << 20, (1u128 << 20) + 1..=u128::MAX],
        ) {
            let wire = format!("RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: {length}\r\n\r\n");
            let err = Reader::default().read(wire.as_bytes()).unwrap_err();
            prop_assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        }
    }
}
