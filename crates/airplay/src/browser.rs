//! Finding receivers: mDNS for `_airplay._tcp` and `_raop._tcp`, a
//! receiver's two records put together by its id. This computer's own
//! AirPlay Receiver, which macOS has on, is left out.

use std::net::IpAddr;

use futures_channel::mpsc::UnboundedReceiver;
use network::mdns::{self, Protocol};

use crate::device::{Device, Record, normalize_id};
use crate::error::{Error, Result};

/// A change in what's found: a receiver added, or its name, address or
/// what it says it does changed, or gone.
pub type Found = network::Found<Device>;

/// Which of `AirPlay::SERVICES` a record is of.
const AIRPLAY: usize = 0;
const RAOP: usize = 1;

/// Receivers, as their records make them.
struct AirPlay;

impl Protocol for AirPlay {
    type Device = Device;

    const NAME: &'static str = "airplay";
    const SERVICES: &'static [&'static str] = &["_airplay._tcp.local.", "_raop._tcp.local."];

    /// The receiver's id a record names: an AirPlay record's `deviceid`,
    /// or the MAC address before the `@` of a RAOP record's name.
    fn id(service: usize, record: &Record) -> Option<String> {
        match service {
            AIRPLAY => record.txt("deviceid").map(normalize_id),
            _ => record
                .instance
                .split_once('@')
                .map(|(mac, _)| normalize_id(mac)),
        }
    }

    fn device(records: &[Option<&Record>]) -> Option<Device> {
        let device = Device::from_records(*records.get(AIRPLAY)?, *records.get(RAOP)?)?;
        // This computer itself: what plays here would play here again.
        (!device.is_at(&this_computer())).then_some(device)
    }
}

/// This computer's addresses on its networks, where its own AirPlay
/// Receiver is found; looked up each time, as they change.
fn this_computer() -> Vec<IpAddr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|interface| !interface.is_loopback())
        .map(|interface| interface.ip())
        .collect()
}

/// Finds receivers in the background, as they come and go.
pub struct Browser(mdns::Browser<AirPlay>);

impl Browser {
    /// Start looking; what's found comes over the channel too.
    pub fn start() -> Result<(Self, UnboundedReceiver<Found>)> {
        let (browser, found) = mdns::Browser::start().map_err(|err| match err {
            mdns::Error::Mdns(err) => Error::protocol(err.to_string()),
            mdns::Error::Io(err) => Error::Io(err),
        })?;
        Ok((Self(browser), found))
    }

    /// The receivers found so far, by id.
    pub fn devices(&self) -> Vec<Device> {
        self.0.devices()
    }
}

#[cfg(test)]
#[path = "tests/browser.rs"]
mod tests;
