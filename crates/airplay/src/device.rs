//! A receiver, as its mDNS records describe it: `_airplay._tcp` for what
//! it is and does, `_raop._tcp` for how it takes sound.

use std::net::{IpAddr, SocketAddr};

pub use network::mdns::Record;

/// What a receiver says it does: the `features` bits, low word first.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Features(pub u64);

impl Features {
    pub const VIDEO: u32 = 0;
    pub const AUDIO: u32 = 9;
    pub const UNIFIED_MEDIA_CONTROL: u32 = 38;
    pub const BUFFERED_AUDIO: u32 = 40;
    pub const PTP: u32 = 41;
    pub const HOMEKIT_PAIRING: u32 = 46;
    pub const COREUTILS_PAIRING: u32 = 48;
    pub const VIDEO_V2: u32 = 49;

    /// From `0xLOW` or `0xLOW,0xHIGH`.
    pub fn parse(text: &str) -> Option<Self> {
        let word = |part: &str| {
            let digits = part
                .trim()
                .strip_prefix("0x")
                .or(part.trim().strip_prefix("0X"))?;
            u32::from_str_radix(digits, 16).ok()
        };
        let mut parts = text.split(',');
        let low = word(parts.next()?)?;
        let high = match parts.next() {
            Some(part) => word(part)?,
            None => 0,
        };
        parts
            .next()
            .is_none()
            .then_some(Self(u64::from(high) << 32 | u64::from(low)))
    }

    pub fn has(self, bit: u32) -> bool {
        self.0 & (1 << bit) != 0
    }
}

/// Status flags (`flags`, `sf`) that say what pairing it wants.
const PIN_REQUIRED: u32 = 0x8;
const PASSWORD_REQUIRED: u32 = 0x80;
const ONE_TIME_PAIRING: u32 = 0x200;

/// How a sender gets to play on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pairing {
    /// Nothing: it plays what it's sent.
    None,
    /// Transient pairing, with the PIN everyone knows.
    Transient,
    /// Pairing once with the PIN it shows; verifying with that after.
    Pin,
    /// A password set on it, which this crate doesn't do.
    Password,
}

/// A receiver found on the network.
#[derive(Clone, PartialEq, Debug)]
pub struct Device {
    /// Its id, which stays as its name and address change: its MAC
    /// address in hex, as `_raop._tcp` names it.
    pub id: String,
    pub name: String,
    pub model: String,
    pub manufacturer: Option<String>,
    /// Where it takes AirPlay, in the order to try: on more than one
    /// network, it has an address on each, and not all reach it from here.
    pub addresses: Vec<SocketAddr>,
    pub features: Features,
    flags: u32,
    /// The audio codecs it takes (`cn`): 0 PCM, 1 ALAC, 2 AAC, 3 AAC-ELD.
    pub codecs: Vec<u8>,
    /// The audio encryptions it takes (`et`): 0 none, 1 RSA, others.
    pub encryptions: Vec<u8>,
    /// Its long-term public key, which pairing confirms.
    pub public_key: Option<[u8; 32]>,
}

/// A MAC address in the form `_raop._tcp` names use: hex, no colons.
pub(crate) fn normalize_id(id: &str) -> String {
    id.chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>()
        .to_uppercase()
}

fn numbers(list: Option<&str>) -> Vec<u8> {
    list.into_iter()
        .flat_map(|list| list.split(','))
        .filter_map(|n| n.trim().parse().ok())
        .collect()
}

fn public_key(hex: Option<&str>) -> Option<[u8; 32]> {
    let hex = hex?;
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect();
    bytes?.try_into().ok()
}

fn flags(text: Option<&str>) -> u32 {
    text.and_then(|text| {
        let digits = text.strip_prefix("0x").unwrap_or(text);
        u32::from_str_radix(digits, 16).ok()
    })
    .unwrap_or(0)
}

impl Device {
    /// The receiver both records describe, or the one there is.
    pub fn from_records(airplay: Option<&Record>, raop: Option<&Record>) -> Option<Self> {
        let main = airplay.or(raop)?;
        let get = |airplay_key: &str, raop_key: &str| {
            airplay
                .and_then(|record| record.txt(airplay_key))
                .or_else(|| raop.and_then(|record| record.txt(raop_key)))
        };
        let raop_name = raop.and_then(|record| record.instance.split_once('@'));
        let id = airplay
            .and_then(|record| record.txt("deviceid"))
            .map(normalize_id)
            .or_else(|| raop_name.map(|(mac, _)| normalize_id(mac)))?;
        let name = match airplay {
            Some(record) => record.instance.clone(),
            None => raop_name.map_or(main.instance.clone(), |(_, name)| name.to_owned()),
        };
        let features = match get("features", "ft") {
            Some(text) => Features::parse(text)?,
            // Older AirPlay 1 speakers, the first AirPort Express among
            // them, don't say: what they do is take sound.
            None if airplay.is_none() => Features(1 << Features::AUDIO),
            None => return None,
        };
        Some(Self {
            id,
            name,
            model: get("model", "am").unwrap_or_default().to_owned(),
            manufacturer: get("manufacturer", "manufacturer").map(str::to_owned),
            addresses: Some(main.socket_addresses()).filter(|addresses| !addresses.is_empty())?,
            features,
            flags: flags(get("flags", "sf")),
            codecs: numbers(raop.and_then(|record| record.txt("cn"))),
            encryptions: numbers(raop.and_then(|record| record.txt("et"))),
            public_key: public_key(get("pk", "pk")),
        })
    }

    /// Whether it takes AirPlay at one of `ips`.
    pub fn is_at(&self, ips: &[IpAddr]) -> bool {
        self.addresses
            .iter()
            .any(|address| ips.contains(&address.ip()))
    }

    /// Whether it shows pictures: a TV, or a box on one. Apple TVs say so
    /// with AirPlay video's first bit; TVs with AirPlay 2 built in, as
    /// LG's, Samsung's and some Google TVs, with only its second.
    pub fn shows_video(&self) -> bool {
        self.features.has(Features::VIDEO) || self.features.has(Features::VIDEO_V2)
    }

    pub fn plays_audio(&self) -> bool {
        self.features.has(Features::AUDIO)
    }

    /// Whether it speaks AirPlay 2, which pairs before anything else.
    pub fn airplay2(&self) -> bool {
        self.features.has(Features::UNIFIED_MEDIA_CONTROL)
            || self.features.has(Features::COREUTILS_PAIRING)
    }

    pub fn pairing(&self) -> Pairing {
        if self.flags & (ONE_TIME_PAIRING | PIN_REQUIRED) != 0 {
            Pairing::Pin
        } else if self.flags & PASSWORD_REQUIRED != 0 {
            Pairing::Password
        } else if self.airplay2() {
            Pairing::Transient
        } else {
            Pairing::None
        }
    }
}

#[cfg(test)]
#[path = "tests/device.rs"]
pub(crate) mod tests;
