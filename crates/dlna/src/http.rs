//! Just enough HTTP/1.1 for UPnP: a request to a device on a connection of
//! its own, and reading what comes over one, a response or a request (the
//! events a device sends back, or in the fake, what's sent to it), which
//! `network`'s reader does. SSDP's datagrams are the same head without a
//! body.

use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

pub use network::http::{Deadline, Head, Message};

/// How long each address has to take a connection.
const CONNECT: Duration = Duration::from_secs(3);

pub const USER_AGENT: &str = "dlna/0.1 UPnP/1.0 DLNADOC/1.50";

/// An `http://` URL.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Url {
    pub host: String,
    pub port: u16,
    /// From the `/` on, with the query.
    pub path: String,
}

/// Whether `text` can go into a request line or header as it is: with no
/// space or control character, which would end it or start another.
pub fn is_clean(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || c.is_whitespace())
}

impl Url {
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.trim();
        if !is_clean(rest) {
            return None;
        }
        let scheme = rest.get(..7)?;
        if !scheme.eq_ignore_ascii_case("http://") {
            return None;
        }
        let rest = &rest[7..];
        let (authority, path) = match rest.find(['/', '?']) {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let path = if path.starts_with('?') {
            format!("/{path}")
        } else {
            path.to_owned()
        };
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let (host, rest) = v6.split_once(']')?;
            (host, rest.strip_prefix(':'))
        } else {
            match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        let port = match port {
            Some(port) => port.parse().ok()?,
            None => 80,
        };
        if host.is_empty() {
            return None;
        }
        Some(Self {
            host: host.to_owned(),
            port,
            path,
        })
    }

    /// `reference` against this URL: absolute, from the root, or beside
    /// this one.
    pub fn join(&self, reference: &str) -> Option<Self> {
        let reference = reference.trim();
        if !is_clean(reference) {
            return None;
        }
        if reference
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
        {
            return Self::parse(reference);
        }
        if let Some(rest) = reference.strip_prefix("//") {
            return Self::parse(&format!("http://{rest}"));
        }
        let path = if reference.starts_with('/') {
            reference.to_owned()
        } else {
            let base = self.path.split(['?', '#']).next().unwrap_or("/");
            let directory = &base[..=base.rfind('/').unwrap_or(0)];
            format!("{directory}{reference}")
        };
        Some(Self {
            path,
            ..self.clone()
        })
    }

    /// Whether it's on the same host as `other`, on whatever port.
    pub fn same_host(&self, other: &Url) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
    }

    /// `host:port`, with brackets for IPv6.
    pub fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn addresses(&self) -> io::Result<Vec<SocketAddr>> {
        Ok((self.host.as_str(), self.port).to_socket_addrs()?.collect())
    }
}

impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "http://{}{}", self.authority(), self.path)
    }
}

/// Send a request and wait up to `timeout` for the whole response.
pub fn request(
    method: &str,
    url: &Url,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> io::Result<Message> {
    let (mut stream, _) = network::connect_first(&url.addresses()?, CONNECT)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut request = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {USER_AGENT}\r\nConnection: close\r\n",
        url.path,
        url.authority()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() || method == "POST" {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    let mut bytes = request.into_bytes();
    bytes.extend_from_slice(body);
    stream.write_all(&bytes)?;
    Message::read(Deadline::new(&stream, timeout), true)
}

/// Write a response; the connection is closed after.
pub fn respond(
    mut stream: &TcpStream,
    status: u16,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        412 => "Precondition Failed",
        500 => "Internal Server Error",
        _ => "",
    };
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nServer: {USER_AGENT}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    stream.write_all(&bytes)?;
    stream.flush()
}

/// The address of this computer that `remote` is reached from.
pub fn local_address_towards(remote: IpAddr) -> io::Result<IpAddr> {
    let socket = match remote {
        IpAddr::V4(_) => UdpSocket::bind("0.0.0.0:0")?,
        IpAddr::V6(_) => UdpSocket::bind("[::]:0")?,
    };
    // No datagram goes: connecting only picks the route.
    socket.connect((remote, 9))?;
    Ok(socket.local_addr()?.ip())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::net::TcpListener;
    use std::thread;
    use std::time::Instant;

    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::select;

    use super::*;

    /// What a URL from a device might be: URLs' pieces put together any
    /// which way, among any characters, line breaks and spaces.
    pub(crate) fn url_text() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            any::<char>().prop_map(String::from),
            select(vec![
                "http://",
                "HTTP://",
                "https://",
                "//",
                "/",
                "?",
                "#",
                ":",
                "[",
                "]",
                "::1",
                "@",
                "%0d%0a",
                "\r\n",
                "\n",
                " ",
                "\t",
                "é",
                "€",
                "192.0.2.10",
                "198.51.100.1",
                "80",
                "65536",
                "+1",
                "..",
                "./",
            ])
            .prop_map(str::to_owned),
        ];
        vec(piece, 0..16).prop_map(|pieces| pieces.concat())
    }

    proptest! {
        #[test]
        fn a_url_read_or_joined_goes_into_a_request_whole(
            text in url_text(),
            reference in url_text(),
        ) {
            let parsed = Url::parse(&text);
            let joined = parsed.as_ref().and_then(|url| url.join(&reference));
            let beside = Url::parse("http://192.0.2.10:9197/dmr/desc.xml")
                .unwrap()
                .join(&reference);
            for url in [parsed, joined, beside].into_iter().flatten() {
                prop_assert!(is_clean(&url.host) && is_clean(&url.path), "{url:?}");
                prop_assert!(url.path.starts_with('/'));
                prop_assert_eq!(Url::parse(&url.to_string()), Some(url));
            }
        }
    }

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
        let (url, server) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello and more");
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
}
