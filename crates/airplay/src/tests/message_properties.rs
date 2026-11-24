use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;

#[hegel::test]
fn messages_read_back_one_after_another_however_they_come(tc: TestCase) {
    let messages = tc.draw(gs::vecs(message()).min_size(1).max_size(3));
    let step = tc.draw(gs::integers::<usize>().min_value(1).max_value(63));
    let wire: Vec<u8> = messages.iter().flat_map(Message::encode).collect();
    let mut stream = Trickle { bytes: &wire, step };
    let mut reader = Reader::default();
    for mut message in messages {
        if !message.body.is_empty() {
            message
                .headers
                .push(("Content-Length".into(), message.body.len().to_string()));
        }
        assert_eq!(reader.read(&mut stream).unwrap(), Some(message));
    }
    assert_eq!(reader.read(&mut stream).unwrap(), None);
}

#[hegel::test]
fn a_content_length_past_a_mebibyte_is_refused_before_it_is_read(tc: TestCase) {
    let past = (1u128 << 20) + 1;
    let length = tc.draw(hegel::one_of!(
        gs::integers::<u128>().min_value(past).max_value(2 << 20),
        gs::integers::<u128>().min_value(past),
    ));
    let wire = format!("RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: {length}\r\n\r\n");
    let err = Reader::default().read(wire.as_bytes()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

/// A message with no `Content-Length`, which `encode` adds for a body.
#[hegel::composite]
fn message(tc: &TestCase) -> Message {
    let name = gs::from_regex("[!-9;-~]{1,12}").filter(|name: &String| {
        !name.eq_ignore_ascii_case("Content-Length")
            && !name.eq_ignore_ascii_case("Transfer-Encoding")
    });
    let value = gs::from_regex("([!-~]{1,8}( [!-~]{1,8}){0,2})?");
    Message {
        start: tc.draw(gs::from_regex("[!-~]{1,10}( [!-~]{1,10}){0,2}")),
        headers: tc.draw(gs::vecs(hegel::tuples!(name, value)).max_size(5)),
        body: tc.draw(gs::binary().max_size(299)),
    }
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
