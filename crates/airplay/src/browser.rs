//! Finding receivers: mDNS for `_airplay._tcp` and `_raop._tcp`, a
//! receiver's two records put together by its id.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use mdns_sd::{ServiceDaemon, ServiceEvent, instance_name_unescaped};

use crate::device::{Device, Record, normalize_id};
use crate::error::{Error, Result};

const AIRPLAY: &str = "_airplay._tcp.local.";
const RAOP: &str = "_raop._tcp.local.";

/// A change in what's found.
#[derive(Clone, PartialEq, Debug)]
pub enum Found {
    Added(Device),
    /// Its name, address or what it says it does changed.
    Changed(Device),
    Removed(Device),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    AirPlay,
    Raop,
}

/// Records by full name, each with when it was resolved, so a receiver's
/// newest record wins.
type Records = HashMap<String, (u64, Record)>;

/// The records found, and the receivers they make.
#[derive(Default)]
struct Directory {
    resolutions: u64,
    airplay: Records,
    raop: Records,
    devices: BTreeMap<String, Device>,
}

/// The receiver's id a record names.
fn id_of(kind: Kind, record: &Record) -> Option<String> {
    match kind {
        Kind::AirPlay => record.txt.get("deviceid").map(|id| normalize_id(id)),
        Kind::Raop => record
            .instance
            .split_once('@')
            .map(|(mac, _)| normalize_id(mac)),
    }
}

impl Directory {
    fn records(&mut self, kind: Kind) -> &mut Records {
        match kind {
            Kind::AirPlay => &mut self.airplay,
            Kind::Raop => &mut self.raop,
        }
    }

    fn resolved(&mut self, kind: Kind, fullname: String, record: Record) -> Option<Found> {
        let id = id_of(kind, &record)?;
        self.resolutions += 1;
        let when = self.resolutions;
        let before = self
            .records(kind)
            .insert(fullname, (when, record))
            .and_then(|(_, old)| id_of(kind, &old));
        // A record that now names another receiver leaves the one before.
        if let Some(old) = before.filter(|old| *old != id) {
            self.refresh(&old);
        }
        self.refresh(&id)
    }

    fn removed(&mut self, kind: Kind, fullname: &str) -> Option<Found> {
        let (_, record) = self.records(kind).remove(fullname)?;
        self.refresh(&id_of(kind, &record)?)
    }

    /// Make the receiver `id` again from its records, saying what changed.
    fn refresh(&mut self, id: &str) -> Option<Found> {
        let find = |records: &Records, kind| {
            records
                .values()
                .filter(|(_, record)| id_of(kind, record).as_deref() == Some(id))
                .max_by_key(|(when, _)| *when)
                .map(|(_, record)| record.clone())
        };
        let airplay = find(&self.airplay, Kind::AirPlay);
        let raop = find(&self.raop, Kind::Raop);
        let device = Device::from_records(airplay.as_ref(), raop.as_ref());
        match (device, self.devices.get(id)) {
            (Some(device), Some(old)) if *old == device => None,
            (Some(device), old) => {
                let added = old.is_none();
                self.devices.insert(id.to_owned(), device.clone());
                Some(if added {
                    Found::Added(device)
                } else {
                    Found::Changed(device)
                })
            }
            (None, Some(_)) => self.devices.remove(id).map(Found::Removed),
            (None, None) => None,
        }
    }
}

/// Finds receivers in the background, as they come and go.
pub struct Browser {
    daemon: ServiceDaemon,
    directory: Arc<Mutex<Directory>>,
    threads: Vec<JoinHandle<()>>,
}

fn lock(directory: &Mutex<Directory>) -> MutexGuard<'_, Directory> {
    directory
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Browser {
    /// Start looking; what's found comes over the channel too.
    pub fn start() -> Result<(Self, UnboundedReceiver<Found>)> {
        let daemon = ServiceDaemon::new().map_err(|err| Error::protocol(err.to_string()))?;
        let directory = Arc::new(Mutex::new(Directory::default()));
        let (sender, receiver) = unbounded();
        let mut threads = Vec::new();
        for (service, kind) in [(AIRPLAY, Kind::AirPlay), (RAOP, Kind::Raop)] {
            let events = daemon
                .browse(service)
                .map_err(|err| Error::protocol(err.to_string()))?;
            let directory = Arc::clone(&directory);
            let sender = sender.clone();
            threads.push(
                thread::Builder::new()
                    .name(format!("airplay browse {service}"))
                    .spawn(move || watch(&events, kind, &directory, &sender))?,
            );
        }
        Ok((
            Self {
                daemon,
                directory,
                threads,
            },
            receiver,
        ))
    }

    /// The receivers found so far, by id.
    pub fn devices(&self) -> Vec<Device> {
        lock(&self.directory).devices.values().cloned().collect()
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // Ends the browse channels, and with them the threads.
        let _ = self.daemon.shutdown();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn watch(
    events: &mdns_sd::Receiver<ServiceEvent>,
    kind: Kind,
    directory: &Mutex<Directory>,
    sender: &UnboundedSender<Found>,
) {
    while let Ok(event) = events.recv() {
        let found = match event {
            ServiceEvent::ServiceResolved(service) => {
                let record = Record {
                    instance: instance_name_unescaped(&service.fullname),
                    addresses: service
                        .addresses
                        .iter()
                        .map(mdns_sd::ScopedIp::to_ip_addr)
                        .collect(),
                    port: service.port,
                    txt: service
                        .txt_properties
                        .iter()
                        .map(|property| (property.key().to_owned(), property.val_str().to_owned()))
                        .collect(),
                };
                lock(directory).resolved(kind, service.fullname.clone(), record)
            }
            ServiceEvent::ServiceRemoved(_, fullname) => lock(directory).removed(kind, &fullname),
            _ => None,
        };
        if let Some(found) = found {
            log::debug!("airplay: {found:?}");
            let _ = sender.unbounded_send(found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::tests::{apple_tv, speaker};

    #[test]
    fn a_receivers_two_records_make_one_device() {
        let mut directory = Directory::default();
        let (airplay, raop) = apple_tv();
        let Some(Found::Added(first)) =
            directory.resolved(Kind::Raop, "raop.tv".into(), raop.clone())
        else {
            panic!("the RAOP record alone is a receiver")
        };
        assert_eq!(first.name, "Living Room");
        assert!(first.codecs.contains(&0));
        let Some(Found::Changed(both)) =
            directory.resolved(Kind::AirPlay, "airplay.tv".into(), airplay.clone())
        else {
            panic!("the AirPlay record adds to it")
        };
        assert_eq!(both.id, first.id);
        assert_eq!(both.public_key, Some([0x11; 32]));
        assert_eq!(directory.devices.len(), 1);
        assert_eq!(
            directory.resolved(Kind::AirPlay, "airplay.tv".into(), airplay),
            None,
            "the same again changes nothing"
        );
    }

    #[test]
    fn a_receiver_goes_once_both_its_records_have() {
        let mut directory = Directory::default();
        let (airplay, raop) = speaker();
        directory.resolved(Kind::AirPlay, "airplay.kitchen".into(), airplay);
        directory.resolved(Kind::Raop, "raop.kitchen".into(), raop);
        let (tv, _) = apple_tv();
        directory.resolved(Kind::AirPlay, "airplay.tv".into(), tv);
        assert_eq!(directory.devices.len(), 2);

        assert!(matches!(
            directory.removed(Kind::AirPlay, "airplay.kitchen"),
            Some(Found::Changed(_))
        ));
        let Some(Found::Removed(gone)) = directory.removed(Kind::Raop, "raop.kitchen") else {
            panic!("gone with its last record")
        };
        assert_eq!(gone.name, "Kitchen");
        assert_eq!(directory.removed(Kind::Raop, "raop.kitchen"), None);
        assert_eq!(directory.devices.len(), 1);
    }

    #[test]
    fn a_renamed_receiver_changes_and_keeps_its_id() {
        let mut directory = Directory::default();
        let (mut airplay, _) = speaker();
        directory.resolved(Kind::AirPlay, "airplay.kitchen".into(), airplay.clone());
        airplay.instance = "Dining Room".into();
        let Some(Found::Changed(renamed)) =
            directory.resolved(Kind::AirPlay, "airplay.dining".into(), airplay)
        else {
            panic!("a new name for the same receiver")
        };
        assert_eq!(renamed.id, "06AABBCCDDEE");
        assert_eq!(renamed.name, "Dining Room");
        assert_eq!(
            directory.removed(Kind::AirPlay, "airplay.kitchen"),
            None,
            "the old name going changes nothing"
        );
        assert_eq!(directory.devices.len(), 1);
    }
}
