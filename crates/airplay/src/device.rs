//! A receiver, as its mDNS records describe it: `_airplay._tcp` for what
//! it is and does, `_raop._tcp` for how it takes sound.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

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
    /// Where it takes AirPlay.
    pub address: SocketAddr,
    pub features: Features,
    flags: u32,
    /// The audio codecs it takes (`cn`): 0 PCM, 1 ALAC, 2 AAC, 3 AAC-ELD.
    pub codecs: Vec<u8>,
    /// The audio encryptions it takes (`et`): 0 none, 1 RSA, others.
    pub encryptions: Vec<u8>,
    /// Its long-term public key, which pairing confirms.
    pub public_key: Option<[u8; 32]>,
}

/// An mDNS service record, resolved.
#[derive(Clone, PartialEq, Debug)]
pub struct Record {
    /// The instance name: `Name` for `_airplay._tcp`, `MAC@Name` for
    /// `_raop._tcp`.
    pub instance: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
    pub txt: HashMap<String, String>,
}

impl Record {
    fn get(&self, key: &str) -> Option<&str> {
        self.txt.get(key).map(String::as_str)
    }

    /// The address to reach it at: IPv4 first, as receivers listen there.
    fn address(&self) -> Option<IpAddr> {
        self.addresses
            .iter()
            .find(|ip| ip.is_ipv4())
            .or(self.addresses.first())
            .copied()
    }
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
                .and_then(|record| record.get(airplay_key))
                .or_else(|| raop.and_then(|record| record.get(raop_key)))
        };
        let raop_name = raop.and_then(|record| record.instance.split_once('@'));
        let id = airplay
            .and_then(|record| record.get("deviceid"))
            .map(normalize_id)
            .or_else(|| raop_name.map(|(mac, _)| normalize_id(mac)))?;
        let name = match airplay {
            Some(record) => record.instance.clone(),
            None => raop_name.map_or(main.instance.clone(), |(_, name)| name.to_owned()),
        };
        let features = Features::parse(get("features", "ft")?)?;
        Some(Self {
            id,
            name,
            model: get("model", "am").unwrap_or_default().to_owned(),
            manufacturer: get("manufacturer", "manufacturer").map(str::to_owned),
            address: SocketAddr::new(main.address()?, main.port),
            features,
            flags: flags(get("flags", "sf")),
            codecs: numbers(raop.and_then(|record| record.get("cn"))),
            encryptions: numbers(raop.and_then(|record| record.get("et"))),
            public_key: public_key(get("pk", "pk")),
        })
    }

    /// Whether it shows pictures: a TV, or a box on one.
    pub fn shows_video(&self) -> bool {
        self.features.has(Features::VIDEO)
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
pub(crate) mod tests {
    use super::*;

    fn record(instance: &str, port: u16, txt: &str) -> Record {
        Record {
            instance: instance.into(),
            addresses: vec!["fe80::1".parse().unwrap(), "192.168.1.20".parse().unwrap()],
            port,
            txt: txt
                .split(' ')
                .filter_map(|pair| pair.split_once('='))
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
        }
    }

    // Records as an Apple TV 4K and a speaker send them, their ids and keys
    // made up.
    pub fn apple_tv() -> (Record, Record) {
        (
            record(
                "Living Room",
                7000,
                "acl=0 deviceid=02:11:22:33:44:55 features=0x4A7FDFD5,0x3C177FDE \
                 flags=0x644 model=AppleTV11,1 protovers=1.1 srcvers=1005.13.2 osvers=27.2.1 \
                 pk=1111111111111111111111111111111111111111111111111111111111111111",
            ),
            record(
                "021122334455@Living Room",
                7000,
                "cn=0,1,2,3 da=true et=0,3,5 ft=0x4A7FDFD5,0x3C177FDE sf=0x644 md=0,1,2 \
                 am=AppleTV11,1 tp=UDP vn=65537 vs=1005.13.2",
            ),
        )
    }

    pub fn speaker() -> (Record, Record) {
        (
            record(
                "Kitchen",
                7000,
                "acl=0 deviceid=06:AA:BB:CC:DD:EE features=0x445F8A00,0x1C340 flags=0x4 \
                 model=Phantom manufacturer=Devialet protovers=1.1 srcvers=366.0 \
                 pk=2222222222222222222222222222222222222222222222222222222222222222",
            ),
            record(
                "06AABBCCDDEE@Kitchen",
                7000,
                "cn=0,1 da=true et=0,4 ft=0x445F8A00,0x1C340 md=0,1,2 sf=0x4 tp=UDP \
                 vn=65537 vs=366.0",
            ),
        )
    }

    #[test]
    fn reads_the_features_low_word_first() {
        let features = Features::parse("0x445F8A00,0x1C340").unwrap();
        assert_eq!(features.0, 0x1C340_445F8A00);
        assert!(features.has(Features::AUDIO));
        assert!(!features.has(Features::VIDEO));
        assert!(features.has(Features::UNIFIED_MEDIA_CONTROL));
        assert_eq!(Features::parse("0x5A7FFFF7").unwrap().0, 0x5A7FFFF7);
        assert_eq!(Features::parse("0x1,0x2").unwrap().0, 0x2_0000_0001);
        assert_eq!(Features::parse("nonsense"), None);
    }

    #[test]
    fn an_apple_tv_shows_video_and_pairs_with_a_pin() {
        let (airplay, raop) = apple_tv();
        let tv = Device::from_records(Some(&airplay), Some(&raop)).unwrap();
        assert_eq!(tv.id, "021122334455");
        assert_eq!(tv.name, "Living Room");
        assert_eq!(tv.model, "AppleTV11,1");
        assert_eq!(tv.address, "192.168.1.20:7000".parse().unwrap());
        assert!(tv.shows_video());
        assert!(tv.plays_audio());
        assert!(tv.airplay2());
        assert_eq!(tv.pairing(), Pairing::Pin);
        assert_eq!(tv.codecs, [0, 1, 2, 3]);
        assert_eq!(tv.public_key, Some([0x11; 32]));
    }

    #[test]
    fn a_speaker_plays_only_sound_and_pairs_for_the_session() {
        let (airplay, raop) = speaker();
        let speaker = Device::from_records(Some(&airplay), Some(&raop)).unwrap();
        assert_eq!(speaker.id, "06AABBCCDDEE");
        assert_eq!(speaker.manufacturer.as_deref(), Some("Devialet"));
        assert!(!speaker.shows_video());
        assert!(speaker.plays_audio());
        assert_eq!(speaker.pairing(), Pairing::Transient);
        assert_eq!(speaker.encryptions, [0, 4]);
    }

    #[test]
    fn a_receiver_with_only_its_raop_record_is_found_too() {
        let raop = record(
            "0A0B0C0D0E0F@Study",
            5000,
            "cn=0,1 et=0,1 ft=0x200 sf=0x0 am=AirPort10,115 tp=UDP",
        );
        let old = Device::from_records(None, Some(&raop)).unwrap();
        assert_eq!(old.id, "0A0B0C0D0E0F");
        assert_eq!(old.name, "Study");
        assert_eq!(old.address.port(), 5000);
        assert!(!old.airplay2());
        assert_eq!(old.pairing(), Pairing::None);
    }
}
