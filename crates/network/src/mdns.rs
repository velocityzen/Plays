//! Finding devices over mDNS: browsing for one or more service types, and
//! the devices their records make. A device may have a record of each
//! type, which its id puts together; the newest of each is the one that
//! counts. What a protocol makes of the records is its `Protocol`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use mdns_sd::{ServiceDaemon, ServiceEvent, instance_name_unescaped};

use crate::{Found, lock};

/// A service record as mDNS gives it, resolved.
#[derive(Clone, PartialEq, Debug)]
pub struct Record {
    /// The instance name, unescaped.
    pub instance: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
    pub txt: HashMap<String, String>,
}

impl Record {
    /// A TXT property, empty or not.
    pub fn txt(&self, key: &str) -> Option<&str> {
        self.txt.get(key).map(String::as_str)
    }

    /// The addresses to try it at: IPv4 first, each kind in order.
    pub fn socket_addresses(&self) -> Vec<SocketAddr> {
        let mut addresses: Vec<SocketAddr> = self
            .addresses
            .iter()
            .map(|ip| SocketAddr::new(*ip, self.port))
            .collect();
        addresses.sort_by_key(|address| (address.is_ipv6(), *address));
        addresses.dedup();
        addresses
    }
}

/// What a protocol browses for, and makes of the records it finds.
pub trait Protocol: 'static {
    type Device: Clone + PartialEq + fmt::Debug + Send;

    /// For logs and the browsing threads' names.
    const NAME: &'static str;
    /// The service types browsed, as `_googlecast._tcp.local.`; records
    /// are told apart by their index here.
    const SERVICES: &'static [&'static str];

    /// The id of the device a record of the `service`th type is for.
    fn id(service: usize, record: &Record) -> Option<String>;

    /// The device that the newest record of each service type for one id
    /// make, in the order of `SERVICES`; `None` where it has none.
    fn device(records: &[Option<&Record>]) -> Option<Self::Device>;
}

/// The records found, each with when it was resolved so a device's newest
/// wins, and the devices they make.
pub struct Directory<P: Protocol> {
    resolutions: u64,
    /// Each service type's records, by full name.
    records: Vec<HashMap<String, (u64, Record)>>,
    devices: BTreeMap<String, P::Device>,
}

impl<P: Protocol> Default for Directory<P> {
    fn default() -> Self {
        Self {
            resolutions: 0,
            records: vec![HashMap::new(); P::SERVICES.len()],
            devices: BTreeMap::new(),
        }
    }
}

impl<P: Protocol> Directory<P> {
    /// A record of the `service`th type resolved, new or again.
    pub fn resolved(
        &mut self,
        service: usize,
        fullname: String,
        record: Record,
    ) -> Option<Found<P::Device>> {
        let id = P::id(service, &record)?;
        self.resolutions += 1;
        let before = self
            .records
            .get_mut(service)?
            .insert(fullname, (self.resolutions, record))
            .and_then(|(_, old)| P::id(service, &old));
        // A record that now names another device leaves the one before.
        if let Some(old) = before.filter(|old| *old != id) {
            self.refresh(&old);
        }
        self.refresh(&id)
    }

    /// A record of the `service`th type gone.
    pub fn removed(&mut self, service: usize, fullname: &str) -> Option<Found<P::Device>> {
        let (_, record) = self.records.get_mut(service)?.remove(fullname)?;
        self.refresh(&P::id(service, &record)?)
    }

    /// The devices, by id.
    pub fn devices(&self) -> Vec<P::Device> {
        self.devices.values().cloned().collect()
    }

    /// Make the device `id` again from its newest records, saying what
    /// changed.
    fn refresh(&mut self, id: &str) -> Option<Found<P::Device>> {
        let newest: Vec<Option<&Record>> = self
            .records
            .iter()
            .enumerate()
            .map(|(service, records)| {
                records
                    .values()
                    .filter(|(_, record)| P::id(service, record).as_deref() == Some(id))
                    .max_by_key(|(when, _)| *when)
                    .map(|(_, record)| record)
            })
            .collect();
        let device = P::device(&newest);
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

/// Why browsing didn't start.
#[derive(Debug)]
pub enum Error {
    /// mDNS itself.
    Mdns(mdns_sd::Error),
    /// A thread to browse on.
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mdns(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Error {}

/// A browser, and the channel what it finds comes over.
type Started<P> = (
    Browser<P>,
    UnboundedReceiver<Found<<P as Protocol>::Device>>,
);

/// Finds devices in the background, as they come and go: a thread for
/// each service type.
pub struct Browser<P: Protocol> {
    daemon: ServiceDaemon,
    directory: Arc<Mutex<Directory<P>>>,
    threads: Vec<JoinHandle<()>>,
}

impl<P: Protocol> Browser<P> {
    /// Start looking; what's found comes over the channel too.
    pub fn start() -> Result<Started<P>, Error> {
        let daemon = ServiceDaemon::new().map_err(Error::Mdns)?;
        let (sender, receiver) = unbounded();
        // Dropped partway, it stops what it started.
        let mut browser = Self {
            daemon,
            directory: Arc::default(),
            threads: Vec::new(),
        };
        for (service, kind) in P::SERVICES.iter().enumerate() {
            let events = browser.daemon.browse(kind).map_err(Error::Mdns)?;
            let directory = Arc::clone(&browser.directory);
            let sender = sender.clone();
            // `chromecast browse`, or with more than one type, `airplay
            // browse _raop._tcp.local.`.
            let name = match P::SERVICES {
                [_] => format!("{} browse", P::NAME),
                _ => format!("{} browse {kind}", P::NAME),
            };
            let thread = thread::Builder::new()
                .name(name)
                .spawn(move || watch(&events, service, &directory, &sender))
                .map_err(Error::Io)?;
            browser.threads.push(thread);
        }
        Ok((browser, receiver))
    }

    /// The devices found so far, by id.
    pub fn devices(&self) -> Vec<P::Device> {
        lock(&self.directory).devices()
    }
}

impl<P: Protocol> Drop for Browser<P> {
    fn drop(&mut self) {
        // Ends the browse channels, and with them the threads.
        let _ = self.daemon.shutdown();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn watch<P: Protocol>(
    events: &mdns_sd::Receiver<ServiceEvent>,
    service: usize,
    directory: &Mutex<Directory<P>>,
    sender: &UnboundedSender<Found<P::Device>>,
) {
    while let Ok(event) = events.recv() {
        let found = match event {
            ServiceEvent::ServiceResolved(resolved) => {
                let record = Record {
                    instance: instance_name_unescaped(&resolved.fullname),
                    addresses: resolved
                        .addresses
                        .iter()
                        .map(mdns_sd::ScopedIp::to_ip_addr)
                        .collect(),
                    port: resolved.port,
                    txt: resolved
                        .txt_properties
                        .iter()
                        .map(|property| (property.key().to_owned(), property.val_str().to_owned()))
                        .collect(),
                };
                lock(directory).resolved(service, resolved.fullname.clone(), record)
            }
            ServiceEvent::ServiceRemoved(_, fullname) => {
                lock(directory).removed(service, &fullname)
            }
            _ => None,
        };
        if let Some(found) = found {
            log::debug!("{}: {found:?}", P::NAME);
            let _ = sender.unbounded_send(found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Devices that are a name, made from one record of the first type, or
    /// one of the second if there's none of the first.
    struct Named;

    impl Protocol for Named {
        type Device = String;
        const NAME: &'static str = "named";
        const SERVICES: &'static [&'static str] = &["_first._tcp.local.", "_second._tcp.local."];

        fn id(_: usize, record: &Record) -> Option<String> {
            record.txt("id").map(str::to_owned)
        }

        fn device(records: &[Option<&Record>]) -> Option<String> {
            let record = records.iter().flatten().next()?;
            Some(record.instance.clone())
        }
    }

    const FIRST: usize = 0;
    const SECOND: usize = 1;

    fn record(id: &str, name: &str) -> Record {
        Record {
            instance: name.into(),
            addresses: vec!["fe80::1".parse().unwrap(), "192.168.1.20".parse().unwrap()],
            port: 7000,
            txt: [("id".to_owned(), id.to_owned())].into(),
        }
    }

    #[test]
    fn addresses_go_ipv4_first_once_each() {
        let mut record = record("a", "A");
        record.addresses.push("192.168.1.10".parse().unwrap());
        record.addresses.push("192.168.1.20".parse().unwrap());
        let addresses: Vec<SocketAddr> =
            ["192.168.1.10:7000", "192.168.1.20:7000", "[fe80::1]:7000"]
                .iter()
                .map(|address| address.parse().unwrap())
                .collect();
        assert_eq!(record.socket_addresses(), addresses);
        assert_eq!(record.txt("id"), Some("a"));
        assert_eq!(record.txt("nothing"), None);
    }

    #[test]
    fn the_newest_record_of_a_type_wins() {
        let mut directory = Directory::<Named>::default();
        assert_eq!(
            directory.resolved(FIRST, "a.1".into(), record("a", "Old")),
            Some(Found::Added("Old".into()))
        );
        assert_eq!(
            directory.resolved(FIRST, "a.2".into(), record("a", "New")),
            Some(Found::Changed("New".into()))
        );
        assert_eq!(
            directory.resolved(SECOND, "a.3".into(), record("a", "Second")),
            None,
            "the first type's still counts first"
        );
        assert_eq!(
            directory.removed(FIRST, "a.2"),
            Some(Found::Changed("Old".into()))
        );
        assert_eq!(
            directory.removed(FIRST, "a.1"),
            Some(Found::Changed("Second".into()))
        );
        assert_eq!(
            directory.removed(SECOND, "a.3"),
            Some(Found::Removed("Second".into()))
        );
        assert!(directory.devices().is_empty());
    }

    #[test]
    fn a_record_that_names_another_device_leaves_the_one_before() {
        let mut directory = Directory::<Named>::default();
        directory.resolved(FIRST, "shared".into(), record("a", "A"));
        assert_eq!(
            directory.resolved(FIRST, "shared".into(), record("b", "B")),
            Some(Found::Added("B".into()))
        );
        assert_eq!(directory.devices(), ["B"], "a has no record left");
        assert_eq!(directory.removed(SECOND, "shared"), None, "of another type");
        assert_eq!(
            directory.resolved(
                FIRST,
                "unnamed".into(),
                Record {
                    txt: HashMap::new(),
                    ..record("", "Nobody")
                }
            ),
            None,
            "a record without an id is no device"
        );
    }
}
