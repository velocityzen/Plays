use super::*;

fn tv() -> Advert {
    Advert {
        udn: "uuid:00000000-0000-1000-8000-0000000000a1".into(),
        kind: MEDIA_RENDERER.into(),
        location: "http://192.0.2.10:9197/dmr".into(),
        max_age: Duration::from_secs(1800),
    }
}

#[test]
fn what_devices_send_reads_back() {
    assert_eq!(
        Message::parse(answer(&tv()).as_bytes()),
        Some(Message::Alive(tv()))
    );
    assert_eq!(
        Message::parse(alive(&tv()).as_bytes()),
        Some(Message::Alive(tv()))
    );
    assert_eq!(
        Message::parse(byebye(&tv().udn, MEDIA_RENDERER).as_bytes()),
        Some(Message::Byebye {
            udn: tv().udn,
            kind: MEDIA_RENDERER.into()
        })
    );
    let search = Message::parse(search(MEDIA_RENDERER, 2).as_bytes()).unwrap();
    assert!(search.searches_for("urn:schemas-upnp-org:device:MediaRenderer:3"));
    assert!(!search.searches_for("urn:schemas-upnp-org:device:MediaServer:1"));
    assert!(
        Message::parse(b"M-SEARCH * HTTP/1.1\r\nST: ssdp:all\r\n\r\n")
            .unwrap()
            .searches_for("anything")
    );
}

#[test]
fn as_a_tv_says_it() {
    let notify = b"NOTIFY * HTTP/1.1\r\n\
            HOST: 239.255.255.250:1900\r\n\
            CACHE-CONTROL: max-age= 1800\r\n\
            LOCATION: http://192.0.2.10:9197/dmr\r\n\
            NT: urn:schemas-upnp-org:service:AVTransport:1\r\n\
            NTS: ssdp:alive\r\n\
            SERVER: SHP, UPnP/1.0, Samsung UPnP SDK/1.0\r\n\
            USN: uuid:00000000-0000-1000-8000-0000000000a1::urn:schemas-upnp-org:service:AVTransport:1\r\n\
            Content-Length: 0\r\n\r\n";
    let Some(Message::Alive(advert)) = Message::parse(notify) else {
        panic!("an advert")
    };
    assert_eq!(advert.udn, tv().udn);
    assert!(!advert.is_renderer(), "about a service, not the device");
    assert_eq!(advert.max_age, Duration::from_secs(1800));
    let bare =
        b"HTTP/1.1 200 OK\r\nLOCATION: http://a/\r\nST: upnp:rootdevice\r\nUSN: uuid:x\r\n\r\n";
    let Some(Message::Alive(advert)) = Message::parse(bare) else {
        panic!("an answer")
    };
    assert_eq!(advert.udn, "uuid:x");
    assert_eq!(advert.max_age, MAX_AGE, "no max-age is half an hour");
    assert_eq!(
        max_age(&Head::parse("X\r\nCache-Control: no-cache, max-age=\"60\"\r\n").unwrap()),
        Duration::from_secs(60)
    );
    assert_eq!(
        max_age(&Head::parse("X\r\nCache-Control: max-age=18446744073709551615\r\n").unwrap()),
        LONGEST_AGE,
        "a day at most"
    );
}

#[test]
fn what_isnt_an_advert() {
    assert_eq!(
        Message::parse(b"HTTP/1.1 404 Not Found\r\nST: x\r\n\r\n"),
        None
    );
    assert_eq!(
        Message::parse(b"NOTIFY * HTTP/1.1\r\nNT: x\r\nNTS: ssdp:update\r\nUSN: y\r\n\r\n"),
        None
    );
    assert_eq!(
        Message::parse(b"NOTIFY * HTTP/1.1\r\nNT: x\r\nNTS: ssdp:alive\r\nUSN: y\r\n\r\n"),
        None,
        "alive without a location"
    );
    assert_eq!(Message::parse(&[0xff, 0xfe]), None);
    assert_eq!(Message::parse(b"GET / HTTP/1.1\r\n\r\n"), None);
}
