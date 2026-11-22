use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;

use super::tests::{Trickle, read};
use super::*;

/// Text with the line breaks, colons and spaces a head is made of.
fn text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        any::<char>().prop_map(String::from),
        Just("\n".to_owned()),
        Just("\r\n".to_owned()),
        Just(":".to_owned()),
        Just(" ".to_owned()),
    ];
    vec(piece, 0..64).prop_map(|pieces| pieces.concat())
}

/// A head's words: no space or line break in them, nor a colon in a
/// header's name, and no `Content-Length` or `Transfer-Encoding`,
/// which `framed` adds.
fn head() -> impl Strategy<Value = Head> {
    let word = "[^\\s]{1,10}";
    let header = (
        "[!-9;-~]{1,12}".prop_filter("framing", |name: &String| {
            !name.eq_ignore_ascii_case("Content-Length")
                && !name.eq_ignore_ascii_case("Transfer-Encoding")
        }),
        vec(word, 0..3).prop_map(|words| words.join(" ")),
    );
    (vec(word, 1..4), vec(header, 0..6)).prop_map(|(start, headers)| Head {
        start: start.join(" "),
        headers,
    })
}

/// `head` and `body` as they'd come: with its length, or in chunks of
/// the sizes given, over and over; its lines ending in CRLF or LF.
fn framed(head: &Head, body: &[u8], chunks: Option<&[usize]>, crlf: bool) -> (Head, Vec<u8>) {
    let end = if crlf { "\r\n" } else { "\n" };
    let mut head = head.clone();
    head.headers.push(match chunks {
        Some(_) => ("Transfer-Encoding".into(), "chunked".into()),
        None => ("Content-Length".into(), body.len().to_string()),
    });
    let mut wire = format!("{}{end}", head.start);
    for (name, value) in &head.headers {
        wire.push_str(&format!("{name}: {value}{end}"));
    }
    wire.push_str(end);
    let mut wire = wire.into_bytes();
    match chunks {
        Some(sizes) => {
            let mut rest = body;
            for size in sizes.iter().cycle() {
                if rest.is_empty() {
                    break;
                }
                let (chunk, after) = rest.split_at((*size).min(rest.len()));
                wire.extend(format!("{:x}{end}", chunk.len()).bytes());
                wire.extend(chunk);
                wire.extend(end.bytes());
                rest = after;
            }
            wire.extend(format!("0{end}{end}").bytes());
        }
        None => wire.extend(body),
    }
    (head, wire)
}

proptest! {
    #[test]
    fn a_head_is_its_lines_trimmed_whatever_the_text(text in text()) {
        if let Some(head) = Head::parse(&text) {
            prop_assert!(!head.start.is_empty());
            prop_assert_eq!(head.start.trim(), &head.start);
            prop_assert!(!head.start.contains('\n'));
            for (name, value) in &head.headers {
                prop_assert_eq!(name.trim(), name);
                prop_assert_eq!(value.trim(), value);
                prop_assert!(!name.contains([':', '\n']) && !value.contains('\n'));
            }
        }
    }

    #[test]
    fn a_message_reads_back_as_it_was_sent_however_the_stream_cuts_it(
        head in head(),
        body in vec(any::<u8>(), 0..300),
        chunks in option::of(vec(1usize..64, 1..6)),
        crlf in any::<bool>(),
        steps in vec(1usize..40, 1..6),
    ) {
        let (head, wire) = framed(&head, &body, chunks.as_deref(), crlf);
        let mut stream = Pieces { bytes: &wire, steps, next: 0 };
        let mut reader = Reader::default();
        let message = reader.read(&mut stream, false).unwrap().unwrap();
        prop_assert_eq!(message, Message { head, body });
        prop_assert_eq!(reader.read(&mut stream, false).unwrap(), None);
    }

    #[test]
    fn a_body_past_the_most_is_refused_before_it_is_read(
        // Just past the most, past it as far as a number goes, and
        // past what a number can be.
        length in prop_oneof![
            (MAX_BODY as u128 + 1)..=(2 * MAX_BODY as u128),
            (MAX_BODY as u128 + 1)..=u128::from(u64::MAX),
            u128::from(u64::MAX)..=u128::MAX,
        ],
        chunks in option::of(vec(1usize..100, 0..4)),
    ) {
        // The stream ends where the body would start, so reading it
        // would end the stream instead.
        let wire = match chunks {
            Some(before) => {
                let mut wire = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_owned();
                for size in before {
                    wire.push_str(&format!("{size:x}\r\n{}\r\n", "x".repeat(size)));
                }
                wire + &format!("{length:x}\r\n")
            }
            None => format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n\r\n"),
        };
        let err = read(wire.as_bytes(), true).unwrap_err();
        prop_assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_head_that_never_ends_is_refused_within_the_most(
        filler in prop_oneof![
            vec(any::<u8>().prop_filter("a line break", |byte| *byte != b'\n'), 1..16),
            vec(prop_oneof![Just(b' '), Just(b'\t'), Just(b'\r'), Just(b'\n')], 1..8)
                .prop_map(|mut blank| { blank.push(b'\n'); blank }),
            "[!-9;-~]{1,8}: [!-~]{0,8}\r?\n".prop_map(String::into_bytes),
        ],
        step in 1usize..2 * READ,
    ) {
        let mut stream = Endless { pattern: filler, step, given: 0 };
        let err = Reader::default().read(&mut stream, true).unwrap_err();
        prop_assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        prop_assert!(stream.given <= MAX_HEAD + READ, "{} bytes read", stream.given);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn what_an_endless_stream_leaves_held_is_bounded(
        pattern in vec(
            prop_oneof![
                any::<u8>().prop_map(|byte| vec![byte]),
                prop_oneof![
                    Just("\r\n"),
                    Just("\n"),
                    Just("Content-Length: 5\r\n"),
                    Just("Transfer-Encoding: chunked\r\n"),
                    Just("ff\r\n"),
                    Just("1;x\r\n"),
                ]
                .prop_map(|text| text.as_bytes().to_vec()),
            ],
            1..12,
        )
        .prop_map(|pieces| pieces.concat()),
        to_end in any::<bool>(),
        step in 512usize..2 * READ,
    ) {
        let mut stream = Endless { pattern, step, given: 0 };
        let mut reader = Reader::default();
        // Each read ends, with a message or an error.
        for _ in 0..4 {
            let Ok(message) = reader.read(&mut stream, to_end) else {
                break;
            };
            let message = message.expect("an endless stream doesn't end");
            prop_assert!(message.body.len() <= MAX_BODY);
            prop_assert!(reader.buffer.capacity() <= 2 * (MAX_HEAD + MAX_BODY + READ));
        }
    }
}

/// Reads the bytes given in pieces of the sizes given, over and over,
/// as a network cuts them.
struct Pieces<'a> {
    bytes: &'a [u8],
    steps: Vec<usize>,
    next: usize,
}

impl Read for Pieces<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let step = self.steps[self.next % self.steps.len()];
        self.next += 1;
        let mut stream = Trickle {
            bytes: self.bytes,
            step,
        };
        let n = stream.read(buf)?;
        self.bytes = stream.bytes;
        Ok(n)
    }
}

/// `pattern` over and over, without end, `step` bytes at a time;
/// counting what it gave, and failing the test past 64 MiB.
struct Endless {
    pattern: Vec<u8>,
    step: usize,
    given: usize,
}

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.step);
        for (i, byte) in buf[..n].iter_mut().enumerate() {
            *byte = self.pattern[(self.given + i) % self.pattern.len()];
        }
        self.given += n;
        assert!(self.given <= 64 << 20, "read on without end");
        Ok(n)
    }
}
