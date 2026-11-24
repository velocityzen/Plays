use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::tests::read_from;
use super::*;

/// What a byte range might say: its pieces any which way, numbers past
/// what a number can be among them.
#[hegel::composite]
fn range_text(tc: &TestCase) -> String {
    let pieces = gs::vecs(gs::sampled_from(vec![
        "0",
        "1",
        "99",
        "-",
        ",",
        " ",
        "+1",
        "x",
        "18446744073709551615",
        "18446744073709551616",
    ]))
    .max_size(5);
    let number = || gs::optional(offset()).map(|n| n.map(|n| n.to_string()).unwrap_or_default());
    let numbers = hegel::tuples!(number(), number(), gs::sampled_from(vec!["", " "]))
        .map(|(first, last, space)| format!("bytes={space}{first}{space}-{space}{last}"));
    tc.draw(hegel::one_of!(
        numbers,
        pieces.map(|pieces| format!("bytes={}", pieces.concat())),
        gs::text(),
    ))
}

/// A length, small enough for ranges to fall either side of it often.
#[hegel::composite]
fn length(tc: &TestCase) -> u64 {
    tc.draw(hegel::one_of!(
        gs::integers::<u64>().max_value(199),
        gs::integers::<u64>()
    ))
}

/// A number a range might give.
#[hegel::composite]
fn offset(tc: &TestCase) -> u64 {
    tc.draw(hegel::one_of!(
        gs::integers::<u64>().max_value(299),
        gs::integers::<u64>()
    ))
}

#[hegel::test]
fn a_range_is_within_what_there_is(tc: TestCase) {
    let header = tc.draw(range_text());
    let len = tc.draw(length());
    if let Ok(Some((start, end))) = range(Some(&header), len) {
        assert!(start <= end && end < len, "{start}-{end} of {len}");
    }
}

#[hegel::test]
fn a_range_is_what_it_asks_for_of_what_there_is(tc: TestCase) {
    let first = tc.draw(gs::optional(offset()));
    let last = tc.draw(gs::optional(offset()));
    let len = tc.draw(length());
    let header = format!(
        "bytes={}-{}",
        first.map(|first| first.to_string()).unwrap_or_default(),
        last.map(|last| last.to_string()).unwrap_or_default(),
    );
    let expected = match (first, last) {
        (None, None) => Err(()),
        // The last `last` bytes.
        (None, Some(0)) => Err(()),
        (None, Some(last)) => len
            .checked_sub(1)
            .map(|end| (len - last.min(len), end))
            .ok_or(()),
        (Some(first), last) => len
            .checked_sub(1)
            .map(|most| (first, last.unwrap_or(most).min(most)))
            .filter(|(first, end)| first <= end)
            .ok_or(()),
    };
    assert_eq!(range(Some(&header), len), expected.map(Some));
}

#[hegel::test]
#[hegel::explicit_test_case(
    method = "GET",
    path = "/".to_owned(),
    query = None::<String>,
    headers = vec![("<".to_owned(), String::new()), ("<".to_owned(), "0".to_owned())],
    http_1_0 = false,
    body = Vec::<u8>::new(),
    blank_before = 0usize,
    crlf = false,
)]
fn a_request_is_read_as_it_was_sent_and_the_next_after_it(tc: TestCase) {
    let method = tc.draw(gs::sampled_from(vec![
        "GET", "HEAD", "OPTIONS", "POST", "BREW",
    ]));
    let path = tc.draw(gs::from_regex(r"/[^\s?]{0,40}"));
    let query = tc.draw(gs::optional(gs::from_regex(r"[^\s]{0,20}")));
    let name = gs::from_regex("[!-9;-~]{1,12}").filter(|name: &String| {
        !name.eq_ignore_ascii_case("content-length") && !name.eq_ignore_ascii_case("connection")
    });
    let value = gs::from_regex("([!-~]{1,8}( [!-~]{1,8}){0,2})?");
    let headers = tc.draw(gs::vecs(hegel::tuples!(name, value)).max_size(5));
    let http_1_0 = tc.draw(gs::booleans());
    let body = tc.draw(gs::binary().max_size(63));
    let blank_before = tc.draw(gs::integers::<usize>().max_value(2));
    let crlf = tc.draw(gs::booleans());

    let end = if crlf { "\r\n" } else { "\n" };
    let target = match &query {
        Some(query) => format!("{path}?{query}"),
        None => path.clone(),
    };
    let version = if http_1_0 { "HTTP/1.0" } else { "HTTP/1.1" };
    let mut wire = end.repeat(blank_before);
    wire.push_str(&format!("{method} {target} {version}{end}"));
    for (name, value) in &headers {
        wire.push_str(&format!("{name}: {value}{end}"));
    }
    wire.push_str(&format!("Content-Length: {}{end}{end}", body.len()));
    let mut wire = wire.into_bytes();
    wire.extend(&body);
    wire.extend(b"GET /next HTTP/1.1\r\n\r\n");

    let mut reader = io::Cursor::new(wire);
    let (request, keep_alive) = read_request(&mut reader).unwrap().unwrap();
    let expected = match method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => Method::Other,
    };
    assert_eq!(request.method, expected);
    assert_eq!(&request.path, &path);
    assert_eq!(&request.query, &query.unwrap_or_default());
    for (name, _) in &headers {
        // A name sent twice, in any case, is the first one sent.
        let first = headers
            .iter()
            .find(|(other, _)| other.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str());
        assert_eq!(request.header(&name.to_ascii_lowercase()), first);
    }
    assert_eq!(keep_alive, !http_1_0);
    let (next, _) = read_request(&mut reader).unwrap().unwrap();
    assert_eq!(next.path, "/next");
    assert!(read_request(&mut reader).unwrap().is_none());
}

#[hegel::test]
fn a_head_that_never_ends_is_refused_having_read_little_past_the_most(tc: TestCase) {
    let (head, filler) = tc.draw(hegel::one_of!(
        // A line without end: the request's, or a header's.
        hegel::tuples!(
            gs::sampled_from(vec!["", "GET / HTTP/1.1\r\n", "GET / HTTP/1.1\r\nX-Long: "]),
            gs::vecs(gs::integers::<u8>().filter(|byte| *byte != b'\n'))
                .min_size(1)
                .max_size(15),
        ),
        // Blank lines before a request that never comes.
        hegel::tuples!(
            gs::just(""),
            gs::vecs(gs::sampled_from(vec![b' ', b'\t', b'\r']))
                .max_size(3)
                .map(|mut blank| {
                    blank.push(b'\n');
                    blank
                }),
        ),
        // Headers without end.
        hegel::tuples!(
            gs::sampled_from(vec!["", "GET / HTTP/1.1\r\n"]),
            gs::from_regex("[!-9;-~]{1,8}: [!-~]{0,8}\r?\n").map(String::into_bytes),
        ),
    ));
    let filler = filler.repeat(4 * MAX_HEAD / filler.len() + 1);
    let (request, read) = read_from(head, io::Cursor::new(filler));
    assert_eq!(
        request.map_err(|err| err.kind()).err(),
        Some(io::ErrorKind::InvalidData)
    );
    assert!(read < 2 * MAX_HEAD, "{} bytes read", read);
}

#[hegel::test]
fn any_bytes_are_read_as_requests_or_refused_without_panicking(tc: TestCase) {
    let mut reader = io::Cursor::new(tc.draw(gs::binary().max_size(1023)));
    for _ in 0..64 {
        if !matches!(read_request(&mut reader), Ok(Some(_))) {
            break;
        }
    }
}
