//! Finding receivers: mDNS for `_airplay._tcp` and `_raop._tcp`, a
//! receiver's two records put together by its id.

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
        Device::from_records(*records.get(AIRPLAY)?, *records.get(RAOP)?)
    }
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
mod tests {
    use network::mdns::Directory;

    use super::*;
    use crate::device::tests::{apple_tv, speaker};

    #[test]
    fn a_receivers_two_records_make_one_device() {
        let mut directory = Directory::<AirPlay>::default();
        let (airplay, raop) = apple_tv();
        let Some(Found::Added(first)) = directory.resolved(RAOP, "raop.tv".into(), raop.clone())
        else {
            panic!("the RAOP record alone is a receiver")
        };
        assert_eq!(first.name, "Living Room");
        assert!(first.codecs.contains(&0));
        let Some(Found::Changed(both)) =
            directory.resolved(AIRPLAY, "airplay.tv".into(), airplay.clone())
        else {
            panic!("the AirPlay record adds to it")
        };
        assert_eq!(both.id, first.id);
        assert_eq!(both.public_key, Some([0x11; 32]));
        assert_eq!(directory.devices().len(), 1);
        assert_eq!(
            directory.resolved(AIRPLAY, "airplay.tv".into(), airplay),
            None,
            "the same again changes nothing"
        );
    }

    #[test]
    fn a_receiver_goes_once_both_its_records_have() {
        let mut directory = Directory::<AirPlay>::default();
        let (airplay, raop) = speaker();
        directory.resolved(AIRPLAY, "airplay.kitchen".into(), airplay);
        directory.resolved(RAOP, "raop.kitchen".into(), raop);
        let (tv, _) = apple_tv();
        directory.resolved(AIRPLAY, "airplay.tv".into(), tv);
        assert_eq!(directory.devices().len(), 2);

        assert!(matches!(
            directory.removed(AIRPLAY, "airplay.kitchen"),
            Some(Found::Changed(_))
        ));
        let Some(Found::Removed(gone)) = directory.removed(RAOP, "raop.kitchen") else {
            panic!("gone with its last record")
        };
        assert_eq!(gone.name, "Kitchen");
        assert_eq!(directory.removed(RAOP, "raop.kitchen"), None);
        assert_eq!(directory.devices().len(), 1);
    }

    #[test]
    fn a_renamed_receiver_changes_and_keeps_its_id() {
        let mut directory = Directory::<AirPlay>::default();
        let (mut airplay, _) = speaker();
        directory.resolved(AIRPLAY, "airplay.kitchen".into(), airplay.clone());
        airplay.instance = "Dining Room".into();
        let Some(Found::Changed(renamed)) =
            directory.resolved(AIRPLAY, "airplay.dining".into(), airplay)
        else {
            panic!("a new name for the same receiver")
        };
        assert_eq!(renamed.id, "06AABBCCDDEE");
        assert_eq!(renamed.name, "Dining Room");
        assert_eq!(
            directory.removed(AIRPLAY, "airplay.kitchen"),
            None,
            "the old name going changes nothing"
        );
        assert_eq!(directory.devices().len(), 1);
    }
}
