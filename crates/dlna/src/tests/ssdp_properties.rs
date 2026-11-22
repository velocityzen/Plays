use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;

use super::*;

/// A datagram an SSDP head might be: a start line and headers that
/// say anything, with the names SSDP reads among them.
fn datagram() -> impl Strategy<Value = String> {
    let start = prop_oneof![
        select(vec![
            "NOTIFY * HTTP/1.1",
            "M-SEARCH * HTTP/1.1",
            "HTTP/1.1 200 OK",
            "HTTP/1.1 404 Not Found",
            "notify * HTTP/1.0",
        ])
        .prop_map(str::to_owned),
        any::<String>(),
    ];
    let value = prop_oneof![
        select(vec![
            "ssdp:alive",
            "ssdp:byebye",
            MEDIA_RENDERER,
            "uuid:a::urn:x",
            "::",
            "max-age=",
            "max-age=99999999999999999999999",
            "max-age=\"1800\", no-cache",
            "http://192.0.2.10/",
        ])
        .prop_map(str::to_owned),
        any::<String>(),
    ];
    let name = prop_oneof![
        select(vec!["NT", "NTS", "USN", "ST", "LOCATION", "CACHE-CONTROL"]).prop_map(str::to_owned),
        any::<String>(),
    ];
    (start, vec((name, value), 0..8)).prop_map(|(start, headers)| {
        let mut datagram = start + "\r\n";
        for (name, value) in headers {
            datagram.push_str(&format!("{name}: {value}\r\n"));
        }
        datagram + "\r\n"
    })
}

proptest! {
    #[test]
    fn any_datagram_is_read_or_left_without_panicking(
        datagram in prop_oneof![
            datagram().prop_map(String::into_bytes),
            vec(any::<u8>(), 0..512),
        ],
    ) {
        if let Some(Message::Alive(advert)) = Message::parse(&datagram) {
            prop_assert!(advert.max_age <= LONGEST_AGE);
        }
    }

    #[test]
    fn an_advert_lasts_as_long_as_it_says_up_to_a_day(
        seconds in prop_oneof![0..=3 * LONGEST_AGE.as_secs(), any::<u64>()],
        quoted in any::<bool>(),
    ) {
        let age = if quoted { format!("\"{seconds}\"") } else { seconds.to_string() };
        let datagram = format!(
            "NOTIFY * HTTP/1.1\r\nCACHE-CONTROL: no-cache, max-age = {age}\r\nNT: {MEDIA_RENDERER}\r\n\
                 NTS: ssdp:alive\r\nUSN: uuid:a::{MEDIA_RENDERER}\r\nLOCATION: http://192.0.2.10/\r\n\r\n"
        );
        let Some(Message::Alive(advert)) = Message::parse(datagram.as_bytes()) else {
            panic!("an advert: {datagram}");
        };
        prop_assert_eq!(advert.max_age, Duration::from_secs(seconds).min(LONGEST_AGE));
    }
}
