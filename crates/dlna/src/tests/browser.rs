use super::*;
use crate::device::Service;
use crate::ssdp::LONGEST_AGE;

fn advert(udn: &str, kind: &str) -> Advert {
    Advert {
        udn: udn.into(),
        kind: kind.into(),
        location: format!("http://192.0.2.10/{udn}.xml"),
        max_age: Duration::from_secs(60),
    }
}

/// `advert` for a renderer at an address of its own, the `n`th.
fn renderer_at(n: usize) -> Advert {
    Advert {
        location: format!("http://10.0.{}.{}/dmr", n / 256, n % 256),
        ..advert(&format!("uuid:tv-{n}"), MEDIA_RENDERER)
    }
}

fn device(name: &str) -> Device {
    Device {
        id: format!("uuid:{name}"),
        name: name.into(),
        manufacturer: String::new(),
        model: String::new(),
        location: String::new(),
        icons: Vec::new(),
        av_transport: Service {
            kind: "urn:schemas-upnp-org:service:AVTransport:1".into(),
            control: "http://192.0.2.10/control".into(),
            events: None,
        },
        rendering_control: None,
        connection_manager: None,
    }
}

#[test]
fn no_more_renderers_are_kept_than_the_most() {
    let mut directory = Directory::default();
    let now = Instant::now();
    let tvs: Vec<_> = (0..=MOST_KEPT).map(renderer_at).collect();
    for tv in &tvs[..MOST_KEPT] {
        assert!(directory.alive(tv, now).is_some());
    }
    let one_more = &tvs[MOST_KEPT];
    assert_eq!(directory.alive(one_more, now), None, "one too many");
    assert_eq!(directory.entries.len(), MOST_KEPT);
    directory.byebye("uuid:tv-0");
    assert!(directory.alive(one_more, now).is_some(), "once one goes");
}

#[test]
fn a_description_too_many_wait_for_is_asked_for_when_next_heard() {
    let mut directory = Directory::default();
    let (describe, asked) = mpsc::sync_channel(1);
    let tv = advert("uuid:tv", MEDIA_RENDERER);
    let speaker = advert("uuid:speaker", MEDIA_RENDERER);
    ask_for(&mut directory, &describe, &tv);
    ask_for(&mut directory, &describe, &speaker);
    assert_eq!(
        asked.try_recv(),
        Ok(("uuid:tv".to_owned(), tv.location.clone()))
    );
    assert!(asked.try_recv().is_err(), "no room for the speaker's");
    ask_for(&mut directory, &describe, &speaker);
    assert_eq!(
        asked.try_recv(),
        Ok(("uuid:speaker".to_owned(), speaker.location.clone())),
        "asked for again once heard again"
    );

    // One described already, moving while there's no room, is asked
    // for again too, and kept meanwhile.
    directory.described("uuid:tv", &tv.location, Ok(device("TV")));
    let moved = Advert {
        location: "http://192.0.2.10/moved.xml".into(),
        ..tv.clone()
    };
    describe.try_send(("busy".into(), String::new())).unwrap();
    ask_for(&mut directory, &describe, &moved);
    assert_eq!(directory.devices(), [device("TV")]);
    asked.try_recv().unwrap();
    ask_for(&mut directory, &describe, &moved);
    assert_eq!(
        asked.try_recv(),
        Ok(("uuid:tv".to_owned(), moved.location.clone()))
    );
}

#[test]
fn renderers_are_described_once_and_last_their_max_age() {
    let mut directory = Directory::default();
    let now = Instant::now();
    let service = advert("uuid:tv", "urn:schemas-upnp-org:service:AVTransport:1");
    assert_eq!(directory.alive(&service, now), None, "not about a renderer");
    let tv = advert("uuid:tv", MEDIA_RENDERER);
    assert_eq!(
        directory.alive(&tv, now),
        Some("http://192.0.2.10/uuid:tv.xml".into())
    );
    assert_eq!(directory.alive(&tv, now), None, "being described");
    assert_eq!(
        directory.described("uuid:tv", &tv.location, Ok(device("TV"))),
        Some(Found::Added(device("TV")))
    );
    assert_eq!(
        directory.described("uuid:tv", &tv.location, Ok(device("TV"))),
        None
    );
    assert_eq!(directory.devices(), [device("TV")]);

    // Any advert of its renews it.
    assert_eq!(
        directory.alive(&service, now + Duration::from_secs(50)),
        None
    );
    assert!(directory.expire(now + Duration::from_secs(100)).is_empty());
    assert_eq!(
        directory.expire(now + Duration::from_secs(110)),
        [Found::Removed(device("TV"))]
    );
    assert!(directory.devices().is_empty());
}

#[test]
fn a_renderer_that_moves_is_described_again() {
    let mut directory = Directory::default();
    let now = Instant::now();
    let tv = advert("uuid:tv", MEDIA_RENDERER);
    directory.alive(&tv, now);
    directory.described("uuid:tv", &tv.location, Ok(device("TV")));
    let moved = Advert {
        location: "http://192.0.2.11/dmr".into(),
        ..tv.clone()
    };
    assert_eq!(directory.alive(&moved, now), Some(moved.location.clone()));
    assert_eq!(
        directory.described("uuid:tv", &tv.location, Ok(device("Old"))),
        None,
        "the old description is stale"
    );
    assert_eq!(
        directory.described("uuid:tv", &moved.location, Ok(device("Renamed"))),
        Some(Found::Changed(device("Renamed")))
    );
    assert_eq!(
        directory.described(
            "uuid:tv",
            &moved.location,
            Err(crate::Error::protocol("no"))
        ),
        None,
        "a failed read keeps what was read"
    );
    assert_eq!(
        directory.byebye("uuid:tv"),
        Some(Found::Removed(device("Renamed")))
    );
    assert_eq!(directory.byebye("uuid:tv"), None);
}

/// A renderer that moves and can't be read there at first, as when it
/// says where it is before it answers there, is kept as it was and
/// read when next heard from.
#[test]
fn a_renderer_that_moves_is_read_again_when_its_first_read_fails() {
    let mut directory = Directory::default();
    let now = Instant::now();
    let tv = advert("uuid:tv", MEDIA_RENDERER);
    directory.alive(&tv, now);
    directory.described("uuid:tv", &tv.location, Ok(device("TV")));
    let moved = Advert {
        location: "http://192.0.2.11/dmr".into(),
        ..tv.clone()
    };
    assert_eq!(directory.alive(&moved, now), Some(moved.location.clone()));
    let failed = Err(crate::Error::protocol("not yet"));
    assert_eq!(
        directory.described("uuid:tv", &moved.location, failed),
        None
    );
    assert_eq!(directory.devices(), [device("TV")], "kept meanwhile");
    assert_eq!(
        directory.alive(&moved, now),
        Some(moved.location.clone()),
        "read again"
    );
    assert_eq!(
        directory.described("uuid:tv", &moved.location, Ok(device("Moved"))),
        Some(Found::Changed(device("Moved")))
    );
    assert_eq!(directory.alive(&moved, now), None, "and not again");
}

#[test]
fn a_description_is_fetched_only_from_where_its_advert_came_from() {
    let notify = b"NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n\
            CACHE-CONTROL: max-age=1800\r\n\
            LOCATION: http://192.0.2.10:9197/dmr\r\nNT: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
            NTS: ssdp:alive\r\nUSN: uuid:tv::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
    let advert = Message::parse(notify).expect("an advert");
    let tv = SocketAddr::from(([192, 0, 2, 10], 1900));
    assert_eq!(from_its_device(advert.clone(), tv), Some(advert.clone()));
    assert_eq!(
        from_its_device(advert, SocketAddr::from(([192, 0, 2, 66], 1900))),
        None
    );
    let byebye = b"NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n\
            NT: urn:schemas-upnp-org:device:MediaRenderer:1\r\nNTS: ssdp:byebye\r\n\
            USN: uuid:tv::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
    let byebye = Message::parse(byebye).expect("a goodbye");
    assert_eq!(
        from_its_device(byebye.clone(), SocketAddr::from(([127, 0, 0, 1], 1900))),
        Some(byebye)
    );
}

/// A renderer at a link-local address, which it gives without a zone or
/// with its own, is reached through the interface it was heard on.
#[test]
fn a_link_local_renderer_is_reached_in_the_zone_it_was_heard_in() {
    let heard = |location: &str, from: &str| {
        let notify = format!(
            "NOTIFY * HTTP/1.1\r\nHOST: [ff02::c]:1900\r\nLOCATION: {location}\r\n\
             NT: {MEDIA_RENDERER}\r\nNTS: ssdp:alive\r\nUSN: uuid:tv::{MEDIA_RENDERER}\r\n\r\n"
        );
        let message = Message::parse(notify.as_bytes()).expect("an advert");
        match from_its_device(message, from.parse().unwrap()) {
            Some(Message::Alive(advert)) => Some(advert.location),
            _ => None,
        }
    };
    let from = "[fe80::1c:2%7]:1900";
    let reached = Some("http://[fe80::1c:2%7]:9197/dmr".to_owned());
    assert_eq!(heard("http://[fe80::1c:2]:9197/dmr", from), reached);
    assert_eq!(heard("http://[fe80::1c:2%eth0]:9197/dmr", from), reached);
    assert_eq!(heard("http://[fe80::1c:3]:9197/dmr", from), None);
    assert_eq!(heard("http://192.0.2.10:9197/dmr", from), None);
    assert_eq!(
        heard("http://[2001:db8::5]:9197/dmr", "[2001:db8::5]:1900"),
        Some("http://[2001:db8::5]:9197/dmr".to_owned()),
        "where there's no zone to it"
    );
}

/// One on IPv4 and IPv6 both is read where it's on IPv4, while it's heard
/// of there, rather than each time it's heard over the other.
#[test]
fn a_renderer_on_both_ip_versions_is_reached_over_ipv4_while_it_is_there() {
    let mut directory = Directory::default();
    let now = Instant::now();
    let v4 = advert("uuid:tv", MEDIA_RENDERER);
    let v6 = Advert {
        location: "http://[fe80::1c:2%7]/dmr".into(),
        ..v4.clone()
    };
    let later = |seconds| now + Duration::from_secs(seconds);
    assert_eq!(directory.alive(&v4, now), Some(v4.location.clone()));
    directory.described("uuid:tv", &v4.location, Ok(device("TV")));
    assert_eq!(directory.alive(&v6, later(30)), None, "kept over IPv4");
    assert_eq!(directory.alive(&v4, later(40)), None);
    assert_eq!(directory.alive(&v6, later(90)), None, "IPv4's renewed");
    assert!(directory.expire(later(100)).is_empty(), "heard over IPv6");
    assert_eq!(
        directory.alive(&v6, later(100)),
        Some(v6.location.clone()),
        "not heard over IPv4 since"
    );
    directory.described("uuid:tv", &v6.location, Ok(device("TV")));
    assert_eq!(
        directory.alive(&v4, later(110)),
        Some(v4.location.clone()),
        "back on IPv4"
    );
    assert_eq!(directory.alive(&v6, later(115)), None, "and kept there");

    let failed = Err(crate::Error::protocol("not there"));
    assert_eq!(directory.described("uuid:tv", &v4.location, failed), None);
    assert_eq!(
        directory.alive(&v6, later(120)),
        Some(v6.location.clone()),
        "read over IPv6 when it can't be over IPv4"
    );

    let mut directory = Directory::default();
    assert_eq!(directory.alive(&v6, now), Some(v6.location.clone()));
    assert_eq!(directory.alive(&v6, now), None);
    assert_eq!(
        directory.alive(&v4, now),
        Some(v4.location.clone()),
        "found on IPv6 first"
    );
}

#[test]
fn an_advert_lasting_for_ever_lasts_a_day() {
    let notify = b"NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n\
            CACHE-CONTROL: max-age=18446744073709551615\r\n\
            LOCATION: http://192.0.2.10/tv.xml\r\nNT: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
            NTS: ssdp:alive\r\nUSN: uuid:tv::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
    let Some(Message::Alive(tv)) = Message::parse(notify) else {
        panic!("an advert")
    };
    let mut directory = Directory::default();
    let now = Instant::now();
    assert!(directory.alive(&tv, now).is_some());
    directory.described("uuid:tv", &tv.location, Ok(device("TV")));
    let day = now + LONGEST_AGE;
    assert!(directory.expire(day - Duration::from_secs(1)).is_empty());
    assert_eq!(directory.expire(day), [Found::Removed(device("TV"))]);
}

#[test]
fn a_renderer_that_cant_be_described_is_tried_again() {
    let mut directory = Directory::default();
    let tv = advert("uuid:tv", MEDIA_RENDERER);
    directory.alive(&tv, Instant::now());
    assert_eq!(
        directory.described("uuid:tv", &tv.location, Err(crate::Error::protocol("no"))),
        None
    );
    assert_eq!(directory.byebye("uuid:tv"), None, "not found, so not gone");
    assert!(directory.alive(&tv, Instant::now()).is_some());
}

/// However many renderers say they're at one address, it's described only
/// so many times a minute; but this computer's own loopback, as a fake
/// renderer's is.
#[test]
fn an_address_is_described_only_so_often() {
    let mut directory = Directory::default();
    let now = Instant::now();
    let at = |udn: &str, host: &str| Advert {
        location: format!("http://{host}:{}/dmr", udn.len()),
        ..advert(udn, MEDIA_RENDERER)
    };
    let tvs: Vec<_> = (0..=DESCRIBED_A_MINUTE)
        .map(|n| at(&format!("uuid:tv-{}", "x".repeat(n)), "192.0.2.10"))
        .collect();
    for tv in &tvs[..DESCRIBED_A_MINUTE] {
        assert!(directory.alive(tv, now).is_some(), "{}", tv.udn);
    }
    let one_more = &tvs[DESCRIBED_A_MINUTE];
    assert_eq!(directory.alive(one_more, now), None, "one too many");
    let elsewhere = at("uuid:elsewhere", "192.0.2.11");
    assert!(
        directory.alive(&elsewhere, now).is_some(),
        "another address"
    );
    let moved = Advert {
        location: "http://192.0.2.10/moved".into(),
        ..elsewhere
    };
    assert_eq!(directory.alive(&moved, now), None, "nor moving there");
    let later = now + MINUTE;
    assert!(directory.alive(one_more, later).is_some(), "a minute on");
    for n in 0..=2 * DESCRIBED_A_MINUTE {
        let fake = at(&format!("uuid:fake-{n}"), "127.0.0.1");
        assert!(directory.alive(&fake, now).is_some(), "on loopback");
    }
    directory.expire(later + MINUTE);
    assert!(directory.addresses.0.is_empty(), "forgotten in time");
}

/// An address whose descriptions fail some times in a row is let be for
/// a while, as one an advert forged to point anywhere gives; one read
/// between failures starts them over.
#[test]
fn an_address_whose_descriptions_keep_failing_is_let_be() {
    let mut directory = Directory::default();
    let now = Instant::now();
    let tv = advert("uuid:tv", MEDIA_RENDERER);
    let fail = |directory: &mut Directory, at| {
        assert_eq!(directory.alive(&tv, at), Some(tv.location.clone()));
        let failed = Err(crate::Error::protocol("no"));
        directory.described("uuid:tv", &tv.location, failed);
    };
    for _ in 1..FAILS {
        fail(&mut directory, now);
    }
    assert_eq!(directory.alive(&tv, now), Some(tv.location.clone()));
    directory.described("uuid:tv", &tv.location, Ok(device("TV")));
    directory.byebye("uuid:tv");
    for _ in 0..FAILS {
        fail(&mut directory, now);
    }
    assert_eq!(directory.alive(&tv, now + REST / 2), None, "let be");
    directory.expire(now + REST / 2);
    assert_eq!(directory.alive(&tv, now + REST / 2), None, "remembered");
    let rested = now + REST;
    assert_eq!(
        directory.alive(&tv, rested),
        Some(tv.location.clone()),
        "described again once rested"
    );
    let failed = Err(crate::Error::protocol("no"));
    directory.described("uuid:tv", &tv.location, failed);
    assert_eq!(
        directory.alive(&tv, rested),
        Some(tv.location.clone()),
        "its failures counted over"
    );
}

/// Only so many addresses are remembered; another isn't described till
/// some are forgotten, once there's nothing to remember of them.
#[test]
fn only_so_many_addresses_are_remembered() {
    let mut directory = Directory::default();
    let now = Instant::now();
    let failed = || Err(crate::Error::protocol("no"));
    for n in 0..MOST_ADDRESSES {
        let tv = renderer_at(n);
        assert!(directory.alive(&tv, now).is_some(), "{}", tv.location);
        directory.described(&tv.udn, &tv.location, failed());
    }
    assert!(directory.entries.is_empty(), "none could be described");
    let one_more = renderer_at(MOST_ADDRESSES);
    assert_eq!(directory.alive(&one_more, now), None, "one too many");
    assert_eq!(
        directory.alive(&one_more, now + MINUTE),
        None,
        "failing ones remembered"
    );
    assert!(
        directory.alive(&one_more, now + REST).is_some(),
        "once forgotten"
    );
    directory.expire(now + REST);
    assert_eq!(directory.addresses.0.len(), 1);
}
