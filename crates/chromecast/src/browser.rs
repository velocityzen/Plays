//! Finding devices: mDNS for `_googlecast._tcp`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use mdns_sd::{ServiceDaemon, ServiceEvent, instance_name_unescaped};

use crate::device::{Device, Record};
use crate::error::{Error, Result};

const SERVICE: &str = "_googlecast._tcp.local.";

/// A change in what's found.
#[derive(Clone, PartialEq, Debug)]
pub enum Found {
    Added(Device),
    /// Its name, address, or what it's showing changed.
    Changed(Device),
    Removed(Device),
}

/// The records found, each with when it was resolved so a device's newest
/// wins, and the devices they make.
#[derive(Default)]
struct Directory {
    resolutions: u64,
    records: HashMap<String, (u64, Record)>,
    devices: BTreeMap<String, Device>,
}

fn id_of(record: &Record) -> Option<String> {
    record.txt.get("id").cloned()
}

impl Directory {
    fn resolved(&mut self, fullname: String, record: Record) -> Option<Found> {
        let id = id_of(&record)?;
        self.resolutions += 1;
        let before = self
            .records
            .insert(fullname, (self.resolutions, record))
            .and_then(|(_, old)| id_of(&old));
        // A record that now names another device leaves the one before.
        if let Some(old) = before.filter(|old| *old != id) {
            self.refresh(&old);
        }
        self.refresh(&id)
    }

    fn removed(&mut self, fullname: &str) -> Option<Found> {
        let (_, record) = self.records.remove(fullname)?;
        self.refresh(&id_of(&record)?)
    }

    /// Make the device `id` again from its newest record, saying what
    /// changed.
    fn refresh(&mut self, id: &str) -> Option<Found> {
        let device = self
            .records
            .values()
            .filter(|(_, record)| id_of(record).as_deref() == Some(id))
            .max_by_key(|(when, _)| *when)
            .and_then(|(_, record)| Device::from_record(record));
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

/// Finds devices in the background, as they come and go.
pub struct Browser {
    daemon: ServiceDaemon,
    directory: Arc<Mutex<Directory>>,
    thread: Option<JoinHandle<()>>,
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
        let events = daemon
            .browse(SERVICE)
            .map_err(|err| Error::protocol(err.to_string()))?;
        let directory = Arc::new(Mutex::new(Directory::default()));
        let (sender, receiver) = unbounded();
        let watched = Arc::clone(&directory);
        let thread = thread::Builder::new()
            .name("chromecast browse".into())
            .spawn(move || watch(&events, &watched, &sender))?;
        Ok((
            Self {
                daemon,
                directory,
                thread: Some(thread),
            },
            receiver,
        ))
    }

    /// The devices found so far, by id.
    pub fn devices(&self) -> Vec<Device> {
        lock(&self.directory).devices.values().cloned().collect()
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // Ends the browse channel, and with it the thread.
        let _ = self.daemon.shutdown();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn watch(
    events: &mdns_sd::Receiver<ServiceEvent>,
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
                lock(directory).resolved(service.fullname.clone(), record)
            }
            ServiceEvent::ServiceRemoved(_, fullname) => lock(directory).removed(&fullname),
            _ => None,
        };
        if let Some(found) = found {
            log::debug!("chromecast: {found:?}");
            let _ = sender.unbounded_send(found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::tests::{google_tv, speaker};

    #[test]
    fn devices_come_change_and_go() {
        let mut directory = Directory::default();
        let Some(Found::Added(tv)) = directory.resolved("tv".into(), google_tv()) else {
            panic!("a new device")
        };
        assert_eq!(
            directory.resolved("tv".into(), google_tv()),
            None,
            "the same again changes nothing"
        );
        let mut idle = google_tv();
        idle.txt.insert("rs".into(), String::new());
        let Some(Found::Changed(changed)) = directory.resolved("tv".into(), idle) else {
            panic!("what it shows changed")
        };
        assert_eq!(changed.id, tv.id);
        assert_eq!(changed.showing, None);

        directory.resolved("kitchen".into(), speaker());
        assert_eq!(directory.devices.len(), 2);
        let Some(Found::Removed(gone)) = directory.removed("tv") else {
            panic!("gone with its record")
        };
        assert_eq!(gone.id, tv.id);
        assert_eq!(directory.removed("tv"), None);
        assert_eq!(directory.devices.len(), 1);
    }

    #[test]
    fn a_renamed_device_changes_and_keeps_its_id() {
        let mut directory = Directory::default();
        directory.resolved("old name".into(), speaker());
        let mut renamed = speaker();
        renamed.txt.insert("fn".into(), "Pantry".into());
        let Some(Found::Changed(device)) = directory.resolved("new name".into(), renamed) else {
            panic!("a new name for the same device")
        };
        assert_eq!(device.name, "Pantry");
        assert_eq!(
            directory.removed("old name"),
            None,
            "the old name going changes nothing"
        );
        assert_eq!(directory.devices.len(), 1);
    }
}
