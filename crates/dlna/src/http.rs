//! Just enough HTTP/1.1 for UPnP: a request to a device on a connection of
//! its own, and reading what comes over one, a response or a request (the
//! events a device sends back, or in the fake, what's sent to it). SSDP's
//! datagrams are the same head without a body.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

/// How long each address has to take a connection.
const CONNECT: Duration = Duration::from_secs(3);
/// The most of a head or a body taken.
const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 8 * 1024 * 1024;

pub const USER_AGENT: &str = "dlna/0.1 UPnP/1.0 DLNADOC/1.50";

/// An `http://` URL.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Url {
    pub host: String,
    pub port: u16,
    /// From the `/` on, with the query.
    pub path: String,
}

impl Url {
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.trim();
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

/// A start line and headers.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Head {
    pub start: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// From its lines, which end with CRLF or LF alone.
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let start = lines.next()?.trim().to_owned();
        if start.is_empty() {
            return None;
        }
        let headers = lines
            .take_while(|line| !line.trim().is_empty())
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_owned(), value.trim().to_owned()))
            })
            .collect();
        Some(Self { start, headers })
    }

    /// A header by its name, whatever its case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// A response's status.
    pub fn status(&self) -> Option<u16> {
        let mut words = self.start.split_whitespace();
        words
            .next()
            .filter(|version| version.starts_with("HTTP/"))?;
        words.next()?.parse().ok()
    }

    /// A request's method and path.
    pub fn request(&self) -> Option<(&str, &str)> {
        let mut words = self.start.split_whitespace();
        let method = words.next()?;
        let path = words.next()?;
        words
            .next()
            .filter(|version| version.starts_with("HTTP/"))?;
        Some((method, path))
    }
}

/// A head and its body.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Message {
    pub head: Head,
    pub body: Vec<u8>,
}

impl Message {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.head.header(name)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Read one off `stream`: a response's body goes on to the end when it
    /// doesn't say how long it is, a request's is then empty.
    pub fn read(stream: impl Read, response: bool) -> io::Result<Self> {
        let mut reader = BufReader::new(stream.take((MAX_HEAD + MAX_BODY) as u64));
        let mut head = String::new();
        loop {
            let start = head.len();
            if reader.read_line(&mut head)? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "closed in the head",
                ));
            }
            if head.len() > MAX_HEAD {
                return Err(io::Error::other("a head too long"));
            }
            // Blank lines before the start line are skipped.
            if head[start..].trim().is_empty() {
                if head[..start].trim().is_empty() {
                    head.clear();
                    continue;
                }
                break;
            }
        }
        let head = Head::parse(&head).ok_or_else(|| io::Error::other("no start line"))?;
        let mut body = Vec::new();
        let chunked = head
            .header("Transfer-Encoding")
            .is_some_and(|coding| coding.to_ascii_lowercase().contains("chunked"));
        if chunked {
            read_chunks(&mut reader, &mut body)?;
        } else if let Some(length) = head.header("Content-Length") {
            let length: usize = length
                .parse()
                .map_err(|_| io::Error::other("a bad Content-Length"))?;
            if length > MAX_BODY {
                return Err(io::Error::other("a body too long"));
            }
            body.resize(length, 0);
            reader.read_exact(&mut body)?;
        } else if response {
            reader.read_to_end(&mut body)?;
        }
        Ok(Self { head, body })
    }
}

fn read_chunks(reader: &mut impl BufRead, body: &mut Vec<u8>) -> io::Result<()> {
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let size = line.trim().split(';').next().unwrap_or_default();
        let size = usize::from_str_radix(size.trim(), 16)
            .map_err(|_| io::Error::other("a bad chunk size"))?;
        if size == 0 {
            // The trailer, to its blank line.
            loop {
                line.clear();
                if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
                    return Ok(());
                }
            }
        }
        // Not `body.len() + size`, which a chunk size could overflow.
        if size > MAX_BODY - body.len() {
            return Err(io::Error::other("a body too long"));
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        line.clear();
        reader.read_line(&mut line)?;
    }
}

/// Connect to the first of `url`'s addresses that takes it.
fn connect(url: &Url) -> io::Result<TcpStream> {
    let mut refused = None;
    for address in url.addresses()? {
        match TcpStream::connect_timeout(&address, CONNECT) {
            Ok(stream) => return Ok(stream),
            Err(err) => refused = Some(err),
        }
    }
    Err(refused.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address")))
}

/// Send a request and wait up to `timeout` for the response.
pub fn request(
    method: &str,
    url: &Url,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> io::Result<Message> {
    let mut stream = connect(url)?;
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
    Message::read(&stream, true)
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
mod tests {
    use std::net::TcpListener;
    use std::thread;

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
    fn heads_have_headers_whatever_their_case() {
        let head = Head::parse(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age = 1800\r\nLocation:http://a/b\r\n\r\n",
        )
        .unwrap();
        assert_eq!(head.status(), Some(200));
        assert_eq!(head.header("cache-control"), Some("max-age = 1800"));
        assert_eq!(head.header("LOCATION"), Some("http://a/b"));
        let search = Head::parse("M-SEARCH * HTTP/1.1\nST: ssdp:all\n").unwrap();
        assert_eq!(search.request(), Some(("M-SEARCH", "*")));
        assert_eq!(search.status(), None);
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
    fn chunks_too_long_are_errors() {
        let mut body = Vec::new();
        let mut huge = &b"5\r\nhello\r\nfffffffffffffffc\r\n"[..];
        assert!(
            read_chunks(&mut huge, &mut body).is_err(),
            "past what counts"
        );
        let mut over = format!("{:x}\r\n", MAX_BODY + 1).into_bytes();
        over.extend([0; 64]);
        assert!(
            read_chunks(&mut over.as_slice(), &mut Vec::new()).is_err(),
            "a byte past the most a body may be"
        );
    }

    #[test]
    fn the_way_to_this_computer() {
        let local = "127.0.0.1".parse().unwrap();
        assert_eq!(local_address_towards(local).unwrap(), local);
    }
}
