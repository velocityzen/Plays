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
mod tests {
    use network::mdns::Directory;

    use super::*;
    use crate::device::tests::{google_tv, speaker};

    /// `_googlecast._tcp`, the one service browsed.
    const SERVICE: usize = 0;

    #[test]
    fn devices_come_change_and_go() {
        let mut directory = Directory::<Cast>::default();
        let Some(Found::Added(tv)) = directory.resolved(SERVICE, "tv".into(), google_tv()) else {
            panic!("a new device")
        };
        assert_eq!(
            directory.resolved(SERVICE, "tv".into(), google_tv()),
            None,
            "the same again changes nothing"
        );
        let mut idle = google_tv();
        idle.txt.insert("rs".into(), String::new());
        let Some(Found::Changed(changed)) = directory.resolved(SERVICE, "tv".into(), idle) else {
            panic!("what it shows changed")
        };
        assert_eq!(changed.id, tv.id);
        assert_eq!(changed.showing, None);

        directory.resolved(SERVICE, "kitchen".into(), speaker());
        assert_eq!(directory.devices().len(), 2);
        let Some(Found::Removed(gone)) = directory.removed(SERVICE, "tv") else {
            panic!("gone with its record")
        };
        assert_eq!(gone.id, tv.id);
        assert_eq!(directory.removed(SERVICE, "tv"), None);
        assert_eq!(directory.devices().len(), 1);
    }

    #[test]
    fn a_renamed_device_changes_and_keeps_its_id() {
        let mut directory = Directory::<Cast>::default();
        directory.resolved(SERVICE, "old name".into(), speaker());
        let mut renamed = speaker();
        renamed.txt.insert("fn".into(), "Pantry".into());
        let Some(Found::Changed(device)) = directory.resolved(SERVICE, "new name".into(), renamed)
        else {
            panic!("a new name for the same device")
        };
        assert_eq!(device.name, "Pantry");
        assert_eq!(
            directory.removed(SERVICE, "old name"),
            None,
            "the old name going changes nothing"
        );
        assert_eq!(directory.devices().len(), 1);
    }
}
