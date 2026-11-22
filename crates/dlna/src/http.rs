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
#[path = "tests/http_properties.rs"]
pub(crate) mod properties;
#[cfg(test)]
#[path = "tests/http.rs"]
pub(crate) mod tests;
