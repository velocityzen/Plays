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
    let tvs: Vec<_> = (0..=MOST_KEPT)
        .map(|n| advert(&format!("uuid:tv-{n}"), MEDIA_RENDERER))
        .collect();
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
    let tv = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
    assert!(from_its_device(&advert, tv));
    assert!(!from_its_device(
        &advert,
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, 66))
    ));
    let byebye = b"NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n\
            NT: urn:schemas-upnp-org:device:MediaRenderer:1\r\nNTS: ssdp:byebye\r\n\
            USN: uuid:tv::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
    let byebye = Message::parse(byebye).expect("a goodbye");
    assert!(from_its_device(&byebye, IpAddr::V4(Ipv4Addr::LOCALHOST)));
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
