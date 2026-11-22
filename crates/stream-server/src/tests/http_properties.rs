use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;
use proptest::sample::select;

use super::tests::read_from;
use super::*;

/// What a byte range might say: its pieces any which way, numbers past
/// what a number can be among them.
fn range_text() -> impl Strategy<Value = String> {
    let piece = select(vec![
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
    ]);
    let number = || option::of(offset()).prop_map(|n| n.map(|n| n.to_string()).unwrap_or_default());
    prop_oneof![
        (number(), number(), select(vec!["", " "]))
            .prop_map(|(first, last, space)| format!("bytes={space}{first}{space}-{space}{last}")),
        vec(piece, 0..6).prop_map(|pieces| format!("bytes={}", pieces.concat())),
        any::<String>(),
    ]
}

/// A length, small enough for ranges to fall either side of it often.
fn length() -> impl Strategy<Value = u64> {
    prop_oneof![0u64..200, any::<u64>()]
}

/// A number a range might give.
fn offset() -> impl Strategy<Value = u64> {
    prop_oneof![0u64..300, any::<u64>()]
}

proptest! {
    #[test]
    fn a_range_is_within_what_there_is(header in range_text(), len in length()) {
        if let Ok(Some((start, end))) = range(Some(&header), len) {
            prop_assert!(start <= end && end < len, "{start}-{end} of {len}");
        }
    }

    #[test]
    fn a_range_is_what_it_asks_for_of_what_there_is(
        first in option::of(offset()),
        last in option::of(offset()),
        len in length(),
    ) {
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
        prop_assert_eq!(range(Some(&header), len), expected.map(Some));
    }

    #[test]
    fn a_request_is_read_as_it_was_sent_and_the_next_after_it(
        method in select(vec!["GET", "HEAD", "OPTIONS", "POST", "BREW"]),
        path in "/[^\\s?]{0,40}",
        query in option::of("[^\\s]{0,20}"),
        headers in vec(
            (
                "[!-9;-~]{1,12}".prop_filter("framing", |name: &String| {
                    !name.eq_ignore_ascii_case("content-length")
                        && !name.eq_ignore_ascii_case("connection")
                }),
                "([!-~]{1,8}( [!-~]{1,8}){0,2})?",
            ),
            0..6,
        ),
        http_1_0 in any::<bool>(),
        body in vec(any::<u8>(), 0..64),
        blank_before in 0usize..3,
        crlf in any::<bool>(),
    ) {
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
        prop_assert_eq!(request.method, expected);
        prop_assert_eq!(&request.path, &path);
        prop_assert_eq!(&request.query, &query.unwrap_or_default());
        for (name, _) in &headers {
            // A name sent twice, in any case, is the first one sent.
            let first = headers
                .iter()
                .find(|(other, _)| other.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str());
            prop_assert_eq!(request.header(&name.to_ascii_lowercase()), first);
        }
        prop_assert_eq!(keep_alive, !http_1_0);
        let (next, _) = read_request(&mut reader).unwrap().unwrap();
        prop_assert_eq!(next.path, "/next");
        prop_assert!(read_request(&mut reader).unwrap().is_none());
    }

    #[test]
    fn a_head_that_never_ends_is_refused_having_read_little_past_the_most(
        (head, filler) in prop_oneof![
            // A line without end: the request's, or a header's.
            (
                select(vec!["", "GET / HTTP/1.1\r\n", "GET / HTTP/1.1\r\nX-Long: "]),
                vec(any::<u8>().prop_filter("a line break", |byte| *byte != b'\n'), 1..16),
            ),
            // Blank lines before a request that never comes.
            (
                Just(""),
                vec(select(vec![b' ', b'\t', b'\r']), 0..4)
                    .prop_map(|mut blank| { blank.push(b'\n'); blank }),
            ),
            // Headers without end.
            (
                select(vec!["", "GET / HTTP/1.1\r\n"]),
                "[!-9;-~]{1,8}: [!-~]{0,8}\r?\n".prop_map(String::into_bytes),
            ),
        ],
    ) {
        let filler = filler.repeat(4 * MAX_HEAD / filler.len() + 1);
        let (request, read) = read_from(head, io::Cursor::new(filler));
        prop_assert_eq!(
            request.map_err(|err| err.kind()).err(),
            Some(io::ErrorKind::InvalidData)
        );
        prop_assert!(read < 2 * MAX_HEAD, "{} bytes read", read);
    }

    #[test]
    fn any_bytes_are_read_as_requests_or_refused_without_panicking(
        bytes in vec(any::<u8>(), 0..1024),
    ) {
        let mut reader = io::Cursor::new(bytes);
        for _ in 0..64 {
            if !matches!(read_request(&mut reader), Ok(Some(_))) {
                break;
            }
        }
    }
}
