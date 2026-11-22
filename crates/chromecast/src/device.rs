//! A Cast device as its `_googlecast._tcp` record describes it.

use std::net::SocketAddr;

pub use network::mdns::Record;

/// What a device does, as its `ca` says.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Capabilities(pub u32);

impl Capabilities {
    pub const VIDEO_OUT: u32 = 1 << 0;
    pub const VIDEO_IN: u32 = 1 << 1;
    pub const AUDIO_OUT: u32 = 1 << 2;
    pub const AUDIO_IN: u32 = 1 << 3;
    pub const DEV_MODE: u32 = 1 << 4;
    /// A group of speakers that play together.
    pub const GROUP: u32 = 1 << 5;

    pub fn has(self, bit: u32) -> bool {
        self.0 & bit != 0
    }
}

/// A device found on the network.
#[derive(Clone, PartialEq, Debug)]
pub struct Device {
    /// Its id, which stays as its name and address change.
    pub id: String,
    pub name: String,
    pub model: String,
    pub capabilities: Capabilities,
    /// Where it takes Cast, in the order to try: on more than one network,
    /// it has an address on each, and not all reach it from here.
    pub addresses: Vec<SocketAddr>,
    /// What it's showing, as it says: an app's name.
    pub showing: Option<String>,
}

/// A TXT property of `record`, unless it's empty, which a device says of
/// what it doesn't have.
fn get<'a>(record: &'a Record, key: &str) -> Option<&'a str> {
    record.txt(key).filter(|value| !value.is_empty())
}

impl Device {
    pub fn from_record(record: &Record) -> Option<Self> {
        let addresses = record.socket_addresses();
        if addresses.is_empty() {
            return None;
        }
        Some(Self {
            id: get(record, "id")?.to_owned(),
            name: get(record, "fn").map_or_else(|| record.instance.clone(), str::to_owned),
            model: get(record, "md").unwrap_or_default().to_owned(),
            capabilities: Capabilities(
                get(record, "ca")
                    .and_then(|ca| ca.parse().ok())
                    .unwrap_or_default(),
            ),
            addresses,
            showing: get(record, "rs").map(str::to_owned),
        })
    }

    /// Whether it shows pictures: a TV, or a stick in one.
    pub fn shows_video(&self) -> bool {
        self.capabilities.has(Capabilities::VIDEO_OUT)
    }

    pub fn plays_audio(&self) -> bool {
        self.capabilities.has(Capabilities::AUDIO_OUT)
    }

    /// Whether it's a group of speakers.
    pub fn is_group(&self) -> bool {
        self.capabilities.has(Capabilities::GROUP)
    }
}

#[cfg(test)]
#[path = "tests/device.rs"]
pub(crate) mod tests;
