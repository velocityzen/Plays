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
#[path = "tests/message_properties.rs"]
mod properties;
#[cfg(test)]
#[path = "tests/message.rs"]
mod tests;
