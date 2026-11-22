use std::net::TcpListener;
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

/// A server that answers one request with `answer`, and hands back the
/// request.
fn serve_once(answer: &'static [u8]) -> (Url, thread::JoinHandle<Message>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = Url::parse(&format!("http://{}/path", listener.local_addr().unwrap())).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = Message::read(&stream, false).unwrap();
        stream.write_all(answer).unwrap();
        request
    });
    (url, server)
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
    let local = "127.0.0.1".parse().unwrap();
    assert_eq!(local_address_towards(local).unwrap(), local);
}
