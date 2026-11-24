use hegel::TestCase;
use hegel::generators as gs;

use super::tests::Trickle;
use super::*;

/// What goes wrong in a head or a body, and line breaks, put anywhere.
const JUNK: &[&str] = &[
    "Content-Length: 5",
    "content-length: +5",
    "Content-Length: -1",
    "Content-Length: 5, 5",
    "Content-Length: 99999999999999999999",
    "Content-Length:",
    "transfer-encoding: gzip, Chunked",
    "ff",
    "FFFFFFFFFFFFFFFFF",
    "5;x=1",
    "0x5",
    ":",
    " ",
    "\t",
    "\r\n",
    "\n",
    "\r",
    "\u{feff}",
];

/// Messages one after another, as a device might send them; now and
/// then with junk put anywhere among them.
fn wire(tc: &TestCase) -> Vec<u8> {
    let mut wire = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let mut message = message(tc);
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
            let junk = if tc.draw(gs::booleans()) {
                tc.draw(gs::sampled_from(JUNK)).as_bytes().to_vec()
            } else {
                tc.draw(gs::binary().min_size(1).max_size(4))
            };
            let at = tc.draw(gs::integers::<usize>().max_value(message.len()));
            message.splice(at..at, junk);
        }
        wire.extend(message);
    }
    wire
}

/// A start line, headers, a blank line and a body by its length, in
/// chunks or to the end, the lines ending in CRLF or LF.
fn message(tc: &TestCase) -> Vec<u8> {
    let end = tc.draw(gs::sampled_from(vec!["\r\n", "\n"]));
    let start = tc.draw(gs::sampled_from(vec![
        "HTTP/1.1 200 OK",
        "RTSP/1.0 200 OK",
        "GET /description.xml HTTP/1.1",
        "NOTIFY * HTTP/1.1",
    ]));
    let mut head = format!("{start}{end}");
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let header = tc.draw(gs::sampled_from(vec![
            "Server: x",
            "CSeq: 1",
            "Content-Type: text/xml",
            "Connection: close",
        ]));
        head.push_str(&format!("{header}{end}"));
    }
    let body = tc.draw(gs::binary().max_size(40));
    let framing = tc.draw(gs::integers::<u8>().max_value(2));
    match framing {
        0 => head.push_str(&format!("Content-Length: {}{end}", body.len())),
        1 => head.push_str(&format!("Transfer-Encoding: chunked{end}")),
        _ => {}
    }
    head.push_str(end);
    let mut message = head.into_bytes();
    if framing == 1 {
        let size = tc.draw(gs::integers::<usize>().min_value(1).max_value(16));
        for chunk in body.chunks(size) {
            message.extend(format!("{:x}{end}", chunk.len()).bytes());
            message.extend(chunk);
            message.extend(end.bytes());
        }
        message.extend(format!("0{end}{end}").bytes());
    } else {
        message.extend(body);
    }
    message
}

/// What a reader makes of `stream`, message by message, to its end or
/// the first error.
fn read_all(mut stream: impl Read, to_end: bool) -> Vec<Result<Message, io::ErrorKind>> {
    let mut reader = Reader::default();
    let mut read = Vec::new();
    loop {
        match reader.read(&mut stream, to_end) {
            Ok(Some(message)) => {
                assert!(message.body.len() <= MAX_BODY);
                read.push(Ok(message));
            }
            Ok(None) => return read,
            Err(err) => {
                read.push(Err(err.kind()));
                return read;
            }
        }
    }
}

#[hegel::test]
fn any_bytes_read_the_same_however_the_stream_cuts_them(tc: TestCase) {
    let wire = wire(&tc);
    let to_end = tc.draw(gs::booleans());
    let step = tc.draw(gs::integers::<usize>().min_value(1).max_value(64));
    let cut = read_all(Trickle { bytes: &wire, step }, to_end);
    assert_eq!(cut, read_all(wire.as_slice(), to_end));
}
