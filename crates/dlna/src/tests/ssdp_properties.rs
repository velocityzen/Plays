use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;

/// A datagram an SSDP head might be: a start line and headers that
/// say anything, with the names SSDP reads among them.
#[hegel::composite]
fn datagram(tc: &TestCase) -> String {
    let start = hegel::one_of!(
        gs::sampled_from(vec![
            "NOTIFY * HTTP/1.1",
            "M-SEARCH * HTTP/1.1",
            "HTTP/1.1 200 OK",
            "HTTP/1.1 404 Not Found",
            "notify * HTTP/1.0",
        ])
        .map(str::to_owned),
        gs::text(),
    );
    let value = hegel::one_of!(
        gs::sampled_from(vec![
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
        .map(str::to_owned),
        gs::text(),
    );
    let name = hegel::one_of!(
        gs::sampled_from(vec!["NT", "NTS", "USN", "ST", "LOCATION", "CACHE-CONTROL"])
            .map(str::to_owned),
        gs::text(),
    );
    let mut datagram = tc.draw(start) + "\r\n";
    for (name, value) in tc.draw(gs::vecs(hegel::tuples!(name, value)).max_size(7)) {
        datagram.push_str(&format!("{name}: {value}\r\n"));
    }
    datagram + "\r\n"
}

#[hegel::test]
fn any_datagram_is_read_or_left_without_panicking(tc: TestCase) {
    let datagram = tc.draw(hegel::one_of!(
        datagram().map(String::into_bytes),
        gs::binary().max_size(511),
    ));
    if let Some(Message::Alive(advert)) = Message::parse(&datagram) {
        assert!(advert.max_age <= LONGEST_AGE);
    }
}

#[hegel::test]
fn an_advert_lasts_as_long_as_it_says_up_to_a_day(tc: TestCase) {
    let seconds = tc.draw(hegel::one_of!(
        gs::integers::<u64>().max_value(3 * LONGEST_AGE.as_secs()),
        gs::integers::<u64>(),
    ));
    let quoted = tc.draw(gs::booleans());
    let age = if quoted {
        format!("\"{seconds}\"")
    } else {
        seconds.to_string()
    };
    let datagram = format!(
        "NOTIFY * HTTP/1.1\r\nCACHE-CONTROL: no-cache, max-age = {age}\r\nNT: {MEDIA_RENDERER}\r\n\
             NTS: ssdp:alive\r\nUSN: uuid:a::{MEDIA_RENDERER}\r\nLOCATION: http://192.0.2.10/\r\n\r\n"
    );
    let Some(Message::Alive(advert)) = Message::parse(datagram.as_bytes()) else {
        panic!("an advert: {datagram}");
    };
    assert_eq!(
        advert.max_age,
        Duration::from_secs(seconds).min(LONGEST_AGE)
    );
}
