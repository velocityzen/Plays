use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::tests::{Trickle, read};
use super::*;

/// Text with the line breaks, colons and spaces a head is made of.
#[hegel::composite]
fn text(tc: &TestCase) -> String {
    let piece = hegel::one_of!(
        gs::characters().map(String::from),
        gs::sampled_from(vec!["\n", "\r\n", ":", " "]).map(str::to_owned),
    );
    tc.draw(gs::vecs(piece).max_size(63)).concat()
}

/// A head's words: no space or line break in them, nor a colon in a
/// header's name, and no `Content-Length` or `Transfer-Encoding`,
/// which `framed` adds.
#[hegel::composite]
fn head(tc: &TestCase) -> Head {
    let word = || gs::from_regex(r"[^\s]{1,10}");
    let name = gs::from_regex("[!-9;-~]{1,12}").filter(|name: &String| {
        !name.eq_ignore_ascii_case("Content-Length")
            && !name.eq_ignore_ascii_case("Transfer-Encoding")
    });
    let value = gs::vecs(word()).max_size(2).map(|words| words.join(" "));
    Head {
        start: tc.draw(gs::vecs(word()).min_size(1).max_size(3)).join(" "),
        headers: tc.draw(gs::vecs(hegel::tuples!(name, value)).max_size(5)),
    }
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

#[hegel::test]
fn a_head_is_its_lines_trimmed_whatever_the_text(tc: TestCase) {
    let text = tc.draw(text());
    if let Some(head) = Head::parse(&text) {
        assert!(!head.start.is_empty());
        assert_eq!(head.start.trim(), &head.start);
        assert!(!head.start.contains('\n'));
        for (name, value) in &head.headers {
            assert_eq!(name.trim(), name);
            assert_eq!(value.trim(), value);
            assert!(!name.contains([':', '\n']) && !value.contains('\n'));
        }
    }
}

#[hegel::test]
fn a_message_reads_back_as_it_was_sent_however_the_stream_cuts_it(tc: TestCase) {
    let head = tc.draw(head());
    let body = tc.draw(gs::binary().max_size(299));
    let chunks = tc.draw(gs::optional(
        gs::vecs(gs::integers::<usize>().min_value(1).max_value(63))
            .min_size(1)
            .max_size(5),
    ));
    let crlf = tc.draw(gs::booleans());
    let steps = tc.draw(
        gs::vecs(gs::integers::<usize>().min_value(1).max_value(39))
            .min_size(1)
            .max_size(5),
    );
    let (head, wire) = framed(&head, &body, chunks.as_deref(), crlf);
    let mut stream = Pieces {
        bytes: &wire,
        steps,
        next: 0,
    };
    let mut reader = Reader::default();
    let message = reader.read(&mut stream, false).unwrap().unwrap();
    assert_eq!(message, Message { head, body });
    assert_eq!(reader.read(&mut stream, false).unwrap(), None);
}

#[hegel::test]
fn a_body_past_the_most_is_refused_before_it_is_read(tc: TestCase) {
    // Just past the most, past it as far as a number goes, and past what
    // a number can be.
    let past = MAX_BODY as u128 + 1;
    let length = tc.draw(hegel::one_of!(
        gs::integers::<u128>()
            .min_value(past)
            .max_value(2 * MAX_BODY as u128),
        gs::integers::<u128>()
            .min_value(past)
            .max_value(u128::from(u64::MAX)),
        gs::integers::<u128>().min_value(u128::from(u64::MAX)),
    ));
    let chunks = tc.draw(gs::optional(
        gs::vecs(gs::integers::<usize>().min_value(1).max_value(99)).max_size(3),
    ));
    // The stream ends where the body would start, so reading it would
    // end the stream instead.
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
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[hegel::test]
fn a_head_that_never_ends_is_refused_within_the_most(tc: TestCase) {
    let filler = tc.draw(hegel::one_of!(
        gs::vecs(gs::integers::<u8>().filter(|byte| *byte != b'\n'))
            .min_size(1)
            .max_size(15),
        gs::vecs(gs::sampled_from(vec![b' ', b'\t', b'\r', b'\n']))
            .min_size(1)
            .max_size(7)
            .map(|mut blank| {
                blank.push(b'\n');
                blank
            }),
        gs::from_regex("[!-9;-~]{1,8}: [!-~]{0,8}\r?\n").map(String::into_bytes),
    ));
    let step = tc.draw(gs::integers::<usize>().min_value(1).max_value(2 * READ - 1));
    let mut stream = Endless {
        pattern: filler,
        step,
        given: 0,
    };
    let err = Reader::default().read(&mut stream, true).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(
        stream.given <= MAX_HEAD + READ,
        "{} bytes read",
        stream.given
    );
}

#[hegel::test(test_cases = 64)]
fn what_an_endless_stream_leaves_held_is_bounded(tc: TestCase) {
    let piece = hegel::one_of!(
        gs::integers::<u8>().map(|byte| vec![byte]),
        gs::sampled_from(vec![
            "\r\n",
            "\n",
            "Content-Length: 5\r\n",
            "Transfer-Encoding: chunked\r\n",
            "ff\r\n",
            "1;x\r\n",
        ])
        .map(|text: &str| text.as_bytes().to_vec()),
    );
    let pattern = tc.draw(gs::vecs(piece).min_size(1).max_size(11)).concat();
    let to_end = tc.draw(gs::booleans());
    let step = tc.draw(
        gs::integers::<usize>()
            .min_value(512)
            .max_value(2 * READ - 1),
    );
    let mut stream = Endless {
        pattern,
        step,
        given: 0,
    };
    let mut reader = Reader::default();
    // Each read ends, with a message or an error.
    for _ in 0..4 {
        let Ok(message) = reader.read(&mut stream, to_end) else {
            break;
        };
        let message = message.expect("an endless stream doesn't end");
        assert!(message.body.len() <= MAX_BODY);
        assert!(reader.buffer.capacity() <= 2 * (MAX_HEAD + MAX_BODY + READ));
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
