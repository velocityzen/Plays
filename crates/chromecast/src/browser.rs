//! Finding devices: mDNS for `_googlecast._tcp`.

use futures_channel::mpsc::UnboundedReceiver;
use network::mdns::{self, Protocol};

use crate::device::{Device, Record};
use crate::error::{Error, Result};

/// A change in what's found: a device added, or its name, address, or what
/// it's showing changed, or gone.
pub type Found = network::Found<Device>;

/// Devices, as their records make them.
struct Cast;

impl Protocol for Cast {
    type Device = Device;

    const NAME: &'static str = "chromecast";
    const SERVICES: &'static [&'static str] = &["_googlecast._tcp.local."];

    fn id(_: usize, record: &Record) -> Option<String> {
        record.txt("id").map(str::to_owned)
    }

    fn device(records: &[Option<&Record>]) -> Option<Device> {
        Device::from_record(records.first().copied().flatten()?)
    }
}

/// Finds devices in the background, as they come and go.
pub struct Browser(mdns::Browser<Cast>);

impl Browser {
    /// Start looking; what's found comes over the channel too.
    pub fn start() -> Result<(Self, UnboundedReceiver<Found>)> {
        let (browser, found) = mdns::Browser::start().map_err(|err| match err {
            mdns::Error::Mdns(err) => Error::protocol(err.to_string()),
            mdns::Error::Io(err) => Error::Io(err),
        })?;
        Ok((Self(browser), found))
    }

    /// The devices found so far, by id.
    pub fn devices(&self) -> Vec<Device> {
        self.0.devices()
    }
}

#[cfg(test)]
#[path = "tests/browser.rs"]
mod tests;
