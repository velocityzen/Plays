//! SSDP, how UPnP devices are found: a search multicast to the group, which
//! devices answer, and the NOTIFYs they multicast as they come and go. Each
//! is a datagram with an HTTP head and no body. There's a group for IPv4,
//! and one for IPv6 on each link.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;

use crate::http::Head;

/// SSDP's group and port.
pub const GROUP: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900);
/// SSDP's IPv6 group on the link, `[ff02::c]:1900`.
pub const GROUP_V6: SocketAddrV6 =
    SocketAddrV6::new(Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xc), 1900, 0, 0);

/// The device type searched for.
pub const MEDIA_RENDERER: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

/// How long an advert lasts when it doesn't say.
const MAX_AGE: Duration = Duration::from_secs(1800);
/// The longest an advert lasts, whatever it says.
pub(crate) const LONGEST_AGE: Duration = Duration::from_secs(24 * 3600);

/// A device there: a NOTIFY `ssdp:alive`, or an answer to a search.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Advert {
    /// The device it's for: the UDN at the start of its USN.
    pub udn: String,
    /// The type it's about, the NT or ST.
    pub kind: String,
    /// Where the device's description is.
    pub location: String,
    /// How long it holds, unless renewed.
    pub max_age: Duration,
}

impl Advert {
    /// About a media renderer, of any version.
    pub fn is_renderer(&self) -> bool {
        is_renderer(&self.kind)
    }
}

fn is_renderer(kind: &str) -> bool {
    kind.starts_with("urn:schemas-upnp-org:device:MediaRenderer:")
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Message {
    /// M-SEARCH, for devices of a type.
    Search {
        target: String,
    },
    Alive(Advert),
    /// NOTIFY `ssdp:byebye`: the device is going.
    Byebye {
        udn: String,
        kind: String,
    },
}

/// `uuid:…` from `uuid:…::urn:…`.
fn udn(usn: &str) -> String {
    usn.split("::").next().unwrap_or_default().trim().to_owned()
}

fn max_age(head: &Head) -> Duration {
    head.header("Cache-Control")
        .into_iter()
        .flat_map(|control| control.split(','))
        .find_map(|directive| {
            let (name, value) = directive.split_once('=')?;
            if !name.trim().eq_ignore_ascii_case("max-age") {
                return None;
            }
            value.trim().trim_matches('"').parse().ok()
        })
        .map_or(MAX_AGE, |seconds| {
            Duration::from_secs(seconds).min(LONGEST_AGE)
        })
}

impl Message {
    pub fn parse(datagram: &[u8]) -> Option<Self> {
        let head = Head::parse(std::str::from_utf8(datagram).ok()?)?;
        let advert = |kind: &str| {
            Some(Advert {
                udn: udn(head.header("USN")?),
                kind: kind.to_owned(),
                location: head.header("Location")?.to_owned(),
                max_age: max_age(&head),
            })
        };
        if let Some(status) = head.status() {
            if status != 200 {
                return None;
            }
            return advert(head.header("ST")?).map(Self::Alive);
        }
        let (method, _) = head.request()?;
        match method.to_ascii_uppercase().as_str() {
            "M-SEARCH" => Some(Self::Search {
                target: head.header("ST")?.to_owned(),
            }),
            "NOTIFY" => {
                let kind = head.header("NT")?;
                match head.header("NTS")? {
                    "ssdp:alive" => advert(kind).map(Self::Alive),
                    "ssdp:byebye" => Some(Self::Byebye {
                        udn: udn(head.header("USN")?),
                        kind: kind.to_owned(),
                    }),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether a device of type `kind` answers it.
    #[cfg(any(test, feature = "fake"))]
    pub fn searches_for(&self, kind: &str) -> bool {
        match self {
            Self::Search { target } => {
                target == "ssdp:all" || target == kind || (is_renderer(target) && is_renderer(kind))
            }
            _ => false,
        }
    }
}

/// A search for `target`, to be answered within `wait` seconds, sent to
/// `group`.
pub fn search(target: &str, wait: u32, group: SocketAddr) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {group}\r\nMAN: \"ssdp:discover\"\r\nMX: {wait}\r\nST: {target}\r\nUSER-AGENT: {}\r\n\r\n",
        crate::http::USER_AGENT
    )
}

/// The answer to a search, as a device gives it.
#[cfg(any(test, feature = "fake"))]
pub fn answer(advert: &Advert) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age={}\r\nEXT:\r\nLOCATION: {}\r\nSERVER: {}\r\nST: {}\r\nUSN: {}::{}\r\n\r\n",
        advert.max_age.as_secs(),
        advert.location,
        crate::http::USER_AGENT,
        advert.kind,
        advert.udn,
        advert.kind
    )
}

/// NOTIFY `ssdp:alive`, as a device multicasts it.
#[cfg(any(test, feature = "fake"))]
pub fn alive(advert: &Advert) -> String {
    format!(
        "NOTIFY * HTTP/1.1\r\nHOST: {GROUP}\r\nCACHE-CONTROL: max-age={}\r\nLOCATION: {}\r\nNT: {}\r\nNTS: ssdp:alive\r\nSERVER: {}\r\nUSN: {}::{}\r\n\r\n",
        advert.max_age.as_secs(),
        advert.location,
        advert.kind,
        crate::http::USER_AGENT,
        advert.udn,
        advert.kind
    )
}

/// NOTIFY `ssdp:byebye`.
#[cfg(any(test, feature = "fake"))]
pub fn byebye(udn: &str, kind: &str) -> String {
    format!(
        "NOTIFY * HTTP/1.1\r\nHOST: {GROUP}\r\nNT: {kind}\r\nNTS: ssdp:byebye\r\nUSN: {udn}::{kind}\r\n\r\n"
    )
}

#[cfg(test)]
#[path = "tests/ssdp_properties.rs"]
mod properties;
#[cfg(test)]
#[path = "tests/ssdp.rs"]
mod tests;
