use std::net::{Ipv6Addr, SocketAddr, TcpListener, ToSocketAddrs};
use std::thread;
use std::time::Instant;

use super::*;

#[test]
fn urls_parse_and_join() {
    let url = Url::parse("http://192.0.2.7:9197/dmr").unwrap();
    assert_eq!(url.host, "192.0.2.7");
    assert_eq!(url.port, 9197);
    assert_eq!(url.path, "/dmr");
    assert_eq!(
        url.join("/upnp/control/AVTransport1").unwrap().to_string(),
        "http://192.0.2.7:9197/upnp/control/AVTransport1"
    );
    let deep = Url::parse("HTTP://192.0.2.7:1990/dd/desc.xml?x=1").unwrap();
    assert_eq!(
        deep.join("AVTransport/control.xml").unwrap().to_string(),
        "http://192.0.2.7:1990/dd/AVTransport/control.xml"
    );
    assert_eq!(
        deep.join("http://192.0.2.8/other").unwrap().to_string(),
        "http://192.0.2.8:80/other"
    );
    assert_eq!(
        url.join("/aaaa€").unwrap().path,
        "/aaaa€",
        "a character across the seventh byte"
    );
    assert_eq!(
        Url::parse("http://[fe80::1]:8080").unwrap().authority(),
        "[fe80::1]:8080"
    );
    assert_eq!(Url::parse("http://host").unwrap().path, "/");
    assert_eq!(Url::parse("http://host?q").unwrap().path, "/?q");
    assert!(Url::parse("https://host/").is_none());
    assert!(Url::parse("http://:80/").is_none());
    assert!(Url::parse("http://host:port/").is_none());
}

#[test]
fn urls_with_spaces_or_line_breaks_are_refused() {
    assert_eq!(Url::parse("http://192.0.2.1/a\r\nX-Evil: 1"), None);
    assert_eq!(Url::parse("http://192.0.2.1/a b"), None);
    assert_eq!(Url::parse("http://192.0.2.1/a\tb"), None);
    let url = Url::parse("  http://192.0.2.1/dmr  ").unwrap();
    assert_eq!(url.path, "/dmr", "only what's around it is trimmed");
    assert_eq!(url.join("/x\r\nHost: elsewhere"), None);
    assert_eq!(url.join("x y"), None);
    assert_eq!(url.join("/upnp/control").unwrap().path, "/upnp/control");
}

#[test]
fn a_host_is_a_name_or_an_address_and_reads_back_as_written() {
    for text in [
        // Its host would be `¡]:`, written `[¡]:]:1`, which reads as nothing.
        "http://¡]::1",
        "http://a:b:80/",
        "http://192.0.2.1]/",
        "http://192.0.2[1/",
        "http://user@192.0.2.1/",
        "http://192.0.2.1#here",
        "http://192.0.2.1\"/",
        "http://192.0.2.1:+80/",
        "http://192.0.2.1:8_0/",
        "http://[192.0.2.1/",
        "http://[host]/",
        "http://[2001:db8::1]x/",
        "http://[2001:db8::1]]:80/",
        "http://[2001:db8::1%]/",
        "http://[2001:db8::1%a@b]/",
        "http://[]/",
    ] {
        assert_eq!(Url::parse(text), None, "{text}");
    }
    for (text, host, port) in [
        ("http://device.local:1400/", "device.local", 1400),
        ("http://my-tv_1.example/", "my-tv_1.example", 80),
        ("http://télé.example/", "télé.example", 80),
        ("http://[2001:db8::1]:8080/x", "2001:db8::1", 8080),
        ("http://[2001:db8::1]/", "2001:db8::1", 80),
        ("http://[::ffff:192.0.2.1]:80/", "::ffff:192.0.2.1", 80),
        // As the events callback's own address, with its scope, writes
        // itself.
        ("http://[fe80::1%5]:49152/events", "fe80::1%5", 49152),
        ("http://[fe80::1%25en0]/", "fe80::1%25en0", 80),
    ] {
        let url = Url::parse(text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!((url.host.as_str(), url.port), (host, port), "{text}");
        assert_eq!(Url::parse(&url.to_string()).as_ref(), Some(&url), "{text}");
    }
}

/// This computer's link-local address on its loopback, which macOS gives
/// it: `fe80::1%lo0`.
#[cfg(target_os = "macos")]
fn link_local() -> SocketAddr {
    ("fe80::1%lo0", 0)
        .to_socket_addrs()
        .unwrap()
        .next()
        .unwrap()
}

/// A server at `at` that answers one request with `answer`, and hands
/// back the request.
fn serve_once_at(at: SocketAddr, answer: &'static [u8]) -> (Url, thread::JoinHandle<Message>) {
    let listener = TcpListener::bind(at).unwrap();
    let url = Url::parse(&format!("http://{}/path", listener.local_addr().unwrap())).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = Message::read(&stream, false).unwrap();
        stream.write_all(answer).unwrap();
        request
    });
    (url, server)
}

fn serve_once(answer: &'static [u8]) -> (Url, thread::JoinHandle<Message>) {
    serve_once_at(([127, 0, 0, 1], 0).into(), answer)
}

#[test]
fn a_request_and_its_response_by_length() {
    let (url, server) = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello and more");
    let response = request(
        "POST",
        &url,
        &[("SOAPAction", "\"urn:x#Play\"")],
        b"<body/>",
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(response.head.status(), Some(200));
    assert_eq!(response.body, b"hello");
    let request = server.join().unwrap();
    assert_eq!(request.head.request(), Some(("POST", "/path")));
    assert_eq!(request.header("soapaction"), Some("\"urn:x#Play\""));
    assert_eq!(request.header("Host"), Some(url.authority().as_str()));
    assert_eq!(request.body, b"<body/>");
}

#[test]
fn a_chunked_response_and_one_to_the_end() {
    let (url, _server) = serve_once(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;x=y\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
    );
    let response = request("GET", &url, &[], &[], Duration::from_secs(2)).unwrap();
    assert_eq!(response.text(), "hello world");
    let (url, _server) = serve_once(b"HTTP/1.0 200 OK\r\n\r\nuntil it closes");
    let response = request("GET", &url, &[], &[], Duration::from_secs(2)).unwrap();
    assert_eq!(response.text(), "until it closes");
}

#[test]
fn an_answer_sent_a_byte_at_a_time_takes_no_longer_than_a_whole_one_may() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = Url::parse(&format!("http://{}/path", listener.local_addr().unwrap())).unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = Message::read(&stream, false);
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n");
        // Each byte well within the wait, but never the whole head.
        for _ in 0..200 {
            thread::sleep(Duration::from_millis(50));
            if stream.write_all(b"X").is_err() {
                return;
            }
        }
    });
    let started = Instant::now();
    let answer = request("GET", &url, &[], &[], Duration::from_millis(500));
    let took = started.elapsed();
    assert!(answer.is_err(), "no whole answer came");
    assert!(took < Duration::from_secs(2), "it took {took:?}");
}

#[test]
fn the_way_to_this_computer() {
    let local = ([127, 0, 0, 1], 0).into();
    assert_eq!(
        local_address_towards(([127, 0, 0, 1], 1900).into()).unwrap(),
        local
    );
    assert_eq!(
        local_address_towards(local).unwrap(),
        local,
        "on no port in particular"
    );
    let local = (Ipv6Addr::LOCALHOST, 0).into();
    assert_eq!(
        local_address_towards((Ipv6Addr::LOCALHOST, 1900).into()).unwrap(),
        local
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_way_to_a_link_local_address_is_in_its_zone() {
    let mut there = link_local();
    there.set_port(1900);
    assert_eq!(local_address_towards(there).unwrap(), link_local());
}

#[test]
fn a_zone_is_this_computers_own() {
    let url = Url::parse("http://[fe80::1c:2%7]:9197/dmr").unwrap();
    assert_eq!(url.host, "fe80::1c:2%7");
    assert_eq!(url.authority(), "[fe80::1c:2%7]:9197");
    assert_eq!(url.host_header(), "[fe80::1c:2]:9197");
    let plain = Url::parse("http://192.0.2.7:9197/dmr").unwrap();
    assert_eq!(plain.host_header(), plain.authority());
    let elsewhere = |text: &str| Url::parse(text).unwrap();
    assert!(url.same_host(&elsewhere("http://[fe80::1c:2]/x")));
    assert!(url.same_host(&elsewhere("http://[FE80::1C:2%eth0]/x")));
    assert!(!url.same_host(&elsewhere("http://[fe80::1c:3%7]/x")));
    assert!(!url.same_host(&plain));
}

/// To a zone, which IPv6's loopback takes and lets be, as a link-local
/// address can't be had here: macOS's on its loopback carries nothing.
#[test]
fn a_request_in_a_zone_goes_there_and_says_no_zone() {
    let (url, server) = serve_once_at(
        (Ipv6Addr::LOCALHOST, 0).into(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
    );
    let url = Url {
        host: "::1%1".into(),
        ..url
    };
    let response = request("GET", &url, &[], &[], Duration::from_secs(2)).unwrap();
    assert_eq!(response.head.status(), Some(200));
    let request = server.join().unwrap();
    assert_eq!(
        request.header("Host"),
        Some(format!("[::1]:{}", url.port).as_str())
    );
}
