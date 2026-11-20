//! SSDP, how UPnP devices are found: a search multicast to the group, which
//! devices answer, and the NOTIFYs they multicast as they come and go. Each
//! is a datagram with an HTTP head and no body.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use crate::http::Head;

/// SSDP's group and port.
pub const GROUP: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900);

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

/// A search for `target`, to be answered within `wait` seconds.
pub fn search(target: &str, wait: u32) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {GROUP}\r\nMAN: \"ssdp:discover\"\r\nMX: {wait}\r\nST: {target}\r\nUSER-AGENT: {}\r\n\r\n",
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
mod tests {
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
}
