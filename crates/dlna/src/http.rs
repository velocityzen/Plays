//! Just enough HTTP/1.1 for UPnP: a request to a device on a connection of
//! its own, and reading what comes over one, a response or a request (the
//! events a device sends back, or in the fake, what's sent to it), which
//! `network`'s reader does. SSDP's datagrams are the same head without a
//! body.

use std::io::{self, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

pub use network::http::{Deadline, Head, Message};

/// How long each address has to take a connection.
const CONNECT: Duration = Duration::from_secs(3);

pub const USER_AGENT: &str = "dlna/0.1 UPnP/1.0 DLNADOC/1.50";

/// An `http://` URL, on a host by name or IPv4 address, or by IPv6 address
/// written in brackets.
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

/// Whether `host`, written without brackets, is a name or an IPv4
/// address: of the characters a host may have (RFC 3986's, and any beyond
/// ASCII), and none that would end it or start what's after it.
fn is_name(host: &str) -> bool {
    !host.is_empty()
        && host
            .chars()
            .all(|c| !c.is_ascii() || c.is_ascii_alphanumeric() || "-._~%!$&'()*+,;=".contains(c))
}

/// Whether `host`, written in brackets, is an IPv6 address, with a zone or
/// without: `fe80::1%5`, as a socket address with a scope writes itself.
fn is_ipv6(host: &str) -> bool {
    let (address, zone) = match host.split_once('%') {
        Some((address, zone)) => (address, Some(zone)),
        None => (host, None),
    };
    address.parse::<Ipv6Addr>().is_ok()
        && zone.is_none_or(|zone| {
            !zone.is_empty()
                && zone
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c))
        })
}

/// A port: digits only, where `u16`'s own parse would take a sign too.
fn parse_port(text: &str) -> Option<u16> {
    if text.bytes().all(|b| b.is_ascii_digit()) {
        text.parse().ok()
    } else {
        None
    }
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
        let (host, port) = match authority.strip_prefix('[') {
            Some(v6) => {
                let (host, rest) = v6.split_once(']')?;
                let port = match rest {
                    "" => None,
                    rest => Some(rest.strip_prefix(':')?),
                };
                (is_ipv6(host).then_some(host)?, port)
            }
            None => {
                let (host, port) = match authority.split_once(':') {
                    Some((host, port)) => (host, Some(port)),
                    None => (authority, None),
                };
                (is_name(host).then_some(host)?, port)
            }
        };
        let port = match port {
            Some(port) => parse_port(port)?,
            None => 80,
        };
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

    /// Whether it's on the same host as `other`, on whatever port, and
    /// in whatever zone: a device gives its own, if any.
    pub fn same_host(&self, other: &Url) -> bool {
        let address = |url: &Url| url.host.split('%').next().unwrap_or_default().to_owned();
        address(self).eq_ignore_ascii_case(&address(other))
    }

    /// `host:port`, with brackets for IPv6.
    pub fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// `host:port` as a request's Host header gives it: without the zone,
    /// which is this computer's own.
    pub fn host_header(&self) -> String {
        match self.host.split_once('%') {
            Some((address, _)) => Self {
                host: address.to_owned(),
                ..self.clone()
            }
            .authority(),
            None => self.authority(),
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
        url.host_header()
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

/// The address of this computer that `remote` is reached from, with the
/// zone it's in where it's link-local, on no port.
pub fn local_address_towards(mut remote: SocketAddr) -> io::Result<SocketAddr> {
    let socket = match remote {
        SocketAddr::V4(_) => UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?,
        SocketAddr::V6(_) => UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0))?,
    };
    // No datagram goes: connecting only picks the route.
    remote.set_port(9);
    socket.connect(remote)?;
    let mut local = socket.local_addr()?;
    local.set_port(0);
    Ok(local)
}

#[cfg(test)]
#[path = "tests/http_properties.rs"]
pub(crate) mod properties;
#[cfg(test)]
#[path = "tests/http.rs"]
pub(crate) mod tests;
