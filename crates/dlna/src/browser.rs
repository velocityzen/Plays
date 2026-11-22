//! Finding renderers: SSDP searches on every interface, and the NOTIFYs
//! renderers multicast as they come and go. Each renderer found is read
//! from its description; one that says byebye, or isn't heard from within
//! the max-age it gave, is dropped.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use network::lock;
use socket2::{Domain, Protocol, SockRef, Socket, Type};

use crate::device::Device;
use crate::error::Result;
use crate::http::Url;
use crate::ssdp::{self, Advert, GROUP, MEDIA_RENDERER, Message};

/// When to search again: soon after the first, as datagrams get lost, then
/// now and then for what missed both.
#[cfg(not(test))]
const SEARCH_AGAIN: Duration = Duration::from_secs(1);
#[cfg(test)]
const SEARCH_AGAIN: Duration = Duration::from_millis(100);
const SEARCH_EVERY: Duration = Duration::from_secs(120);
/// How long renderers may wait to answer a search, in seconds.
const SEARCH_WAIT: u32 = 2;
/// How often the threads look up.
#[cfg(not(test))]
const TICK: Duration = Duration::from_millis(250);
#[cfg(test)]
const TICK: Duration = Duration::from_millis(10);

/// The most renderers kept; adverts for others are let be until some go.
const MOST_KEPT: usize = 256;
/// How many descriptions are read at once, and how many more may wait.
const DESCRIBERS: usize = 4;
const WAITING: usize = 64;

/// A change in what's found: a renderer added, or its description
/// changed, or gone.
pub type Found = network::Found<Device>;

struct Entry {
    location: String,
    expires: Instant,
    /// Once its description is read.
    device: Option<Device>,
}

/// The renderers heard of, by the UDN their adverts give.
#[derive(Default)]
struct Directory {
    entries: HashMap<String, Entry>,
}

impl Directory {
    /// An advert heard; says where to read a description from, if one's
    /// needed.
    fn alive(&mut self, advert: &Advert, now: Instant) -> Option<String> {
        let expires = now + advert.max_age;
        let full = self.entries.len() >= MOST_KEPT;
        match self.entries.get_mut(&advert.udn) {
            Some(entry) => {
                entry.expires = entry.expires.max(expires);
                let moved = advert.is_renderer() && entry.location != advert.location;
                moved.then(|| {
                    entry.location.clone_from(&advert.location);
                    advert.location.clone()
                })
            }
            None if advert.is_renderer() && full => {
                log::debug!("dlna: {MOST_KEPT} renderers kept; not {}", advert.udn);
                None
            }
            None if advert.is_renderer() => {
                self.entries.insert(
                    advert.udn.clone(),
                    Entry {
                        location: advert.location.clone(),
                        expires,
                        device: None,
                    },
                );
                Some(advert.location.clone())
            }
            None => None,
        }
    }

    /// The description from `location` read, or not.
    fn described(&mut self, udn: &str, location: &str, device: Result<Device>) -> Option<Found> {
        let entry = self.entries.get_mut(udn)?;
        if entry.location != location {
            return None;
        }
        match device {
            Ok(device) if entry.device.as_ref() == Some(&device) => None,
            Ok(device) => {
                let found = if entry.device.is_some() {
                    Found::Changed(device.clone())
                } else {
                    Found::Added(device.clone())
                };
                entry.device = Some(device);
                Some(found)
            }
            Err(err) => {
                log::debug!("dlna: {location}: {err}");
                // Tried again when it's next heard from; one read before
                // is kept as it was meanwhile.
                if entry.device.is_some() {
                    entry.location.clear();
                } else {
                    self.entries.remove(udn);
                }
                None
            }
        }
    }

    /// Its description couldn't be asked for: it is asked for again when
    /// it's next heard from.
    fn forget(&mut self, udn: &str) {
        match self.entries.get_mut(udn) {
            Some(entry) if entry.device.is_some() => entry.location.clear(),
            Some(_) => {
                self.entries.remove(udn);
            }
            None => {}
        }
    }

    fn byebye(&mut self, udn: &str) -> Option<Found> {
        self.entries.remove(udn)?.device.map(Found::Removed)
    }

    fn expire(&mut self, now: Instant) -> Vec<Found> {
        let over: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.expires <= now)
            .map(|(udn, _)| udn.clone())
            .collect();
        over.iter().filter_map(|udn| self.byebye(udn)).collect()
    }

    fn devices(&self) -> Vec<Device> {
        let mut devices: Vec<Device> = self
            .entries
            .values()
            .filter_map(|entry| entry.device.clone())
            .collect();
        devices.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        devices
    }
}

/// Where searches go, and adverts come from.
#[derive(Clone, Copy)]
enum Network {
    /// SSDP's group, on every interface.
    Multicast,
    /// One address, as a fake renderer's.
    #[cfg(any(test, feature = "fake"))]
    At(SocketAddr),
}

/// The IPv4 interfaces that are up, but this computer's own.
fn interfaces() -> Vec<Ipv4Addr> {
    let mut interfaces: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|interface| !interface.is_loopback() && interface.is_oper_up())
        .filter_map(|interface| match interface.ip() {
            std::net::IpAddr::V4(ip) => Some(ip),
            std::net::IpAddr::V6(_) => None,
        })
        .collect();
    interfaces.sort();
    interfaces.dedup();
    interfaces
}

/// SSDP's port, shared with whatever else listens on it.
fn group_listener() -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, GROUP.port())).into())?;
    Ok(socket.into())
}

enum Input {
    Heard(Message),
    Described {
        udn: String,
        location: String,
        device: Box<Result<Device>>,
    },
    Search,
    Stop,
}

/// What the browsing thread has.
struct Browsing {
    network: Network,
    search: UdpSocket,
    /// SSDP's port, when it could be had.
    listen: Option<UdpSocket>,
    /// The interfaces the group was joined on.
    joined: HashSet<Ipv4Addr>,
    directory: Arc<Mutex<Directory>>,
    /// Where the describers take the descriptions to read from.
    describe: mpsc::SyncSender<Described>,
    found: UnboundedSender<Found>,
}

/// A renderer's UDN, and where its description is.
type Described = (String, String);

/// `DESCRIBERS` threads reading the descriptions asked for, at most
/// `WAITING` of them waiting, each read told to `inputs`. They end once
/// what's asked of them is dropped.
fn describers(inputs: &mpsc::Sender<Input>) -> io::Result<mpsc::SyncSender<Described>> {
    let (asks, asked) = mpsc::sync_channel::<Described>(WAITING);
    let asked = Arc::new(Mutex::new(asked));
    for _ in 0..DESCRIBERS {
        let asked = Arc::clone(&asked);
        let inputs = inputs.clone();
        thread::Builder::new()
            .name("dlna describe".into())
            .spawn(move || {
                loop {
                    let Ok((udn, location)) = lock(&asked).recv() else {
                        return;
                    };
                    let device = Box::new(Device::describe(&location));
                    let described = Input::Described {
                        udn,
                        location,
                        device,
                    };
                    if inputs.send(described).is_err() {
                        return;
                    }
                }
            })?;
    }
    Ok(asks)
}

/// Have `advert`'s description read if it needs to be, or, with too many
/// waiting already, forget it until it's heard from again.
fn ask_for(directory: &mut Directory, describe: &mpsc::SyncSender<Described>, advert: &Advert) {
    let Some(location) = directory.alive(advert, Instant::now()) else {
        return;
    };
    if let Err(err) = describe.try_send((advert.udn.clone(), location)) {
        log::debug!("dlna: {} not described now: {err}", advert.udn);
        directory.forget(&advert.udn);
    }
}

impl Browsing {
    fn search(&mut self) {
        let search = ssdp::search(MEDIA_RENDERER, SEARCH_WAIT);
        match self.network {
            Network::Multicast => {
                let interfaces = interfaces();
                for interface in &interfaces {
                    if let Some(listen) = &self.listen
                        && !self.joined.contains(interface)
                    {
                        match listen.join_multicast_v4(GROUP.ip(), interface) {
                            Ok(()) => {
                                self.joined.insert(*interface);
                            }
                            Err(err) => log::debug!("dlna: joining on {interface}: {err}"),
                        }
                    }
                    let sent = SockRef::from(&self.search)
                        .set_multicast_if_v4(interface)
                        .and_then(|()| self.search.send_to(search.as_bytes(), GROUP));
                    if let Err(err) = sent {
                        log::debug!("dlna: searching on {interface}: {err}");
                    }
                }
            }
            #[cfg(any(test, feature = "fake"))]
            Network::At(address) => {
                if let Err(err) = self.search.send_to(search.as_bytes(), address) {
                    log::debug!("dlna: searching at {address}: {err}");
                }
            }
        }
    }

    fn tell(&self, found: Found) {
        log::debug!("dlna: {found:?}");
        let _ = self.found.unbounded_send(found);
    }

    fn heard(&self, message: Message) {
        let mut directory = lock(&self.directory);
        match message {
            Message::Alive(advert) => ask_for(&mut directory, &self.describe, &advert),
            Message::Byebye { udn, .. } => {
                if let Some(found) = directory.byebye(&udn) {
                    drop(directory);
                    self.tell(found);
                }
            }
            Message::Search { .. } => {}
        }
    }

    fn run(mut self, inputs: &mpsc::Receiver<Input>) {
        let started = Instant::now();
        let mut searched_again = false;
        let mut searched = started;
        self.search();
        loop {
            match inputs.recv_timeout(TICK) {
                Ok(Input::Heard(message)) => self.heard(message),
                Ok(Input::Described {
                    udn,
                    location,
                    device,
                }) => {
                    let found = lock(&self.directory).described(&udn, &location, *device);
                    if let Some(found) = found {
                        self.tell(found);
                    }
                }
                Ok(Input::Search) => {
                    searched = Instant::now();
                    self.search();
                }
                Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            let expired = lock(&self.directory).expire(now);
            for found in expired {
                self.tell(found);
            }
            if (!searched_again && now >= started + SEARCH_AGAIN) || now >= searched + SEARCH_EVERY
            {
                searched_again = true;
                searched = now;
                self.search();
            }
        }
    }
}

/// Whether `message` came from where it says the device is: a description
/// is fetched only from the address the advert came from, not wherever it
/// points.
fn from_its_device(message: &Message, from: IpAddr) -> bool {
    match message {
        Message::Alive(advert) => Url::parse(&advert.location)
            .and_then(|url| url.host.parse::<IpAddr>().ok())
            .is_some_and(|host| host == from),
        _ => true,
    }
}

/// Read datagrams off `socket` until `stop`.
fn hear(socket: &UdpSocket, stop: &AtomicBool, inputs: &mpsc::Sender<Input>) {
    let mut buffer = [0; 8 * 1024];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buffer) {
            Ok((n, from)) => {
                if let Some(message) = Message::parse(&buffer[..n]) {
                    log::trace!("dlna: from {from}: {message:?}");
                    if !from_its_device(&message, from.ip()) {
                        log::debug!("dlna: {from} told of a description elsewhere");
                        continue;
                    }
                    if inputs.send(Input::Heard(message)).is_err() {
                        return;
                    }
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(err) => {
                log::debug!("dlna: {err}");
                thread::sleep(TICK);
            }
        }
    }
}

/// Finds renderers in the background, as they come and go.
pub struct Browser {
    directory: Arc<Mutex<Directory>>,
    inputs: mpsc::Sender<Input>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    #[cfg(any(test, feature = "fake"))]
    listening: Option<SocketAddr>,
}

impl Browser {
    /// Start looking on every interface; what's found comes over the
    /// channel too.
    pub fn start() -> Result<(Self, UnboundedReceiver<Found>)> {
        Self::start_on(Network::Multicast)
    }

    /// Start looking for the fake renderer that answers at `search`; the
    /// adverts it sends go to [`Self::listening_at`].
    #[cfg(any(test, feature = "fake"))]
    pub fn start_at(search: SocketAddr) -> Result<(Self, UnboundedReceiver<Found>)> {
        Self::start_on(Network::At(search))
    }

    /// Where adverts are heard, as the fake renderer sends them.
    #[cfg(any(test, feature = "fake"))]
    pub fn listening_at(&self) -> Option<SocketAddr> {
        self.listening
    }

    fn start_on(network: Network) -> Result<(Self, UnboundedReceiver<Found>)> {
        let (search, listen) = match network {
            Network::Multicast => {
                let search = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
                // As UPnP asks, to cross a router or so.
                search.set_multicast_ttl_v4(2)?;
                let listen = group_listener()
                    .inspect_err(|err| {
                        log::warn!("dlna: no NOTIFYs, as SSDP's port can't be had: {err}");
                    })
                    .ok();
                (search, listen)
            }
            #[cfg(any(test, feature = "fake"))]
            Network::At(_) => (
                UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?,
                Some(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?),
            ),
        };
        #[cfg(any(test, feature = "fake"))]
        let listening = listen.as_ref().and_then(|listen| listen.local_addr().ok());
        let directory = Arc::new(Mutex::new(Directory::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (inputs, received) = mpsc::channel();
        let (found, receiver) = unbounded();
        let mut threads = Vec::new();
        for socket in [Some(&search), listen.as_ref()].into_iter().flatten() {
            socket.set_read_timeout(Some(TICK))?;
            let socket = socket.try_clone()?;
            let stop = Arc::clone(&stop);
            let inputs = inputs.clone();
            threads.push(
                thread::Builder::new()
                    .name("dlna listen".into())
                    .spawn(move || hear(&socket, &stop, &inputs))?,
            );
        }
        let browsing = Browsing {
            network,
            search,
            listen,
            joined: HashSet::new(),
            directory: Arc::clone(&directory),
            describe: describers(&inputs)?,
            found,
        };
        threads.push(
            thread::Builder::new()
                .name("dlna browse".into())
                .spawn(move || browsing.run(&received))?,
        );
        Ok((
            Self {
                directory,
                inputs,
                stop,
                threads,
                #[cfg(any(test, feature = "fake"))]
                listening,
            },
            receiver,
        ))
    }

    /// The renderers found so far, by name.
    pub fn devices(&self) -> Vec<Device> {
        lock(&self.directory).devices()
    }

    /// Search again now, as for a list being opened.
    pub fn search(&self) {
        let _ = self.inputs.send(Input::Search);
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.inputs.send(Input::Stop);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
#[path = "tests/browser.rs"]
mod tests;
