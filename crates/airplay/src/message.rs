//! HTTP and RTSP messages, which AirPlay mixes on one connection: a start
//! line, headers, and a body as long as `Content-Length` says.

use std::io::{self, Read};

/// Longest head taken, so a receiver sending nonsense can't fill memory.
const MAX_HEAD: usize = 64 * 1024;

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

/// Reads messages off a stream, keeping what it read past the last one.
#[derive(Default)]
pub struct Reader {
    buffer: Vec<u8>,
}

impl Reader {
    /// The next message; `None` once the stream ends between messages.
    pub fn read(&mut self, stream: &mut impl Read) -> io::Result<Option<Message>> {
        let end = loop {
            if let Some(end) = self.buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                break end;
            }
            if self.buffer.len() > MAX_HEAD {
                return Err(invalid("a message head that doesn't end"));
            }
            if !self.fill(stream)? {
                return if self.buffer.is_empty() {
                    Ok(None)
                } else {
                    Err(io::ErrorKind::UnexpectedEof.into())
                };
            }
        };
        let head = String::from_utf8_lossy(&self.buffer[..end]).into_owned();
        let mut lines = head.split("\r\n");
        let start = lines.next().unwrap_or_default().to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_owned(), value.trim().to_owned()))
            })
            .collect();
        let length = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("Content-Length"))
            .map(|(_, value)| value.parse::<usize>())
            .transpose()
            .map_err(|_| invalid("a Content-Length that isn't a number"))?
            .unwrap_or(0);
        let body_start = end + 4;
        while self.buffer.len() < body_start + length {
            if !self.fill(stream)? {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        let body = self.buffer[body_start..body_start + length].to_vec();
        self.buffer.drain(..body_start + length);
        Ok(Some(Message {
            start,
            headers,
            body,
        }))
    }

    /// Read more; false at the end of the stream.
    fn fill(&mut self, stream: &mut impl Read) -> io::Result<bool> {
        let mut chunk = [0; 4096];
        let n = stream.read(&mut chunk)?;
        self.buffer.extend_from_slice(&chunk[..n]);
        Ok(n > 0)
    }
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

#[cfg(test)]
mod tests {
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
}
