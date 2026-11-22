use proptest::collection::vec;
use proptest::prelude::*;

use super::*;

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
