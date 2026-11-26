//! Finding renderers: SSDP searches on every interface, over IPv4 and
//! IPv6, and the NOTIFYs renderers multicast as they come and go. Each
//! renderer found is read from its description; one that says byebye, or
//! isn't heard from within the max-age it gave, is dropped.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket};
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
use crate::ssdp::{self, Advert, GROUP, GROUP_V6, MEDIA_RENDERER, Message};

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
    /// When `location` lapses, unless it's heard of again: till then it's
    /// kept over one at an IPv6 address.
    location_expires: Instant,
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
                if !advert.is_renderer() {
                    return None;
                }
                if entry.location == advert.location {
                    entry.location_expires = entry.location_expires.max(expires);
                    return None;
                }
                // A renderer on IPv4 and IPv6 both is reached over IPv4
                // while it's heard of there, not flipped between them.
                let kept = !entry.location.is_empty()
                    && entry.location_expires > now
                    && over_ipv6(&advert.location)
                    && !over_ipv6(&entry.location);
                if kept {
                    return None;
                }
                entry.location.clone_from(&advert.location);
                entry.location_expires = expires;
                Some(advert.location.clone())
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
                        location_expires: expires,
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

/// Whether a description is at an IPv6 address.
fn over_ipv6(location: &str) -> bool {
    Url::parse(location).is_some_and(|url| url.host.contains(':'))
}

/// The interfaces that are up, but this computer's own.
#[derive(Default)]
struct Interfaces {
    /// Their IPv4 addresses.
    v4: Vec<Ipv4Addr>,
    /// The indexes of those with IPv6.
    v6: Vec<u32>,
}

fn interfaces() -> Interfaces {
    let mut found = Interfaces::default();
    for interface in if_addrs::get_if_addrs().unwrap_or_default() {
        if interface.is_loopback() || !interface.is_oper_up() {
            continue;
        }
        match interface.ip() {
            IpAddr::V4(ip) => found.v4.push(ip),
            IpAddr::V6(_) => found.v6.extend(interface.index),
        }
    }
    found.v4.sort();
    found.v4.dedup();
    found.v6.sort_unstable();
    found.v6.dedup();
    found
}

/// SSDP's port on `any`, IPv4's or IPv6's, shared with whatever else
/// listens on it.
fn group_listener(any: IpAddr) -> io::Result<UdpSocket> {
    let socket = Socket::new(
        Domain::for_address(SocketAddr::new(any, 0)),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;
    if any.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::new(any, GROUP.port()).into())?;
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
    /// Searching over IPv6, when this computer has it.
    search_v6: Option<UdpSocket>,
    /// SSDP's port, when it could be had.
    listen: Option<UdpSocket>,
    listen_v6: Option<UdpSocket>,
    /// The interfaces the group was joined on.
    joined: HashSet<Ipv4Addr>,
    /// The interfaces IPv6's group was joined on, by index.
    joined_v6: HashSet<u32>,
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
        match self.network {
            Network::Multicast => {
                let interfaces = interfaces();
                let search = ssdp::search(MEDIA_RENDERER, SEARCH_WAIT, GROUP.into());
                for interface in &interfaces.v4 {
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
                let Some(socket) = &self.search_v6 else {
                    return;
                };
                let search = ssdp::search(MEDIA_RENDERER, SEARCH_WAIT, GROUP_V6.into());
                for &index in &interfaces.v6 {
                    if let Some(listen) = &self.listen_v6
                        && !self.joined_v6.contains(&index)
                    {
                        match listen.join_multicast_v6(GROUP_V6.ip(), index) {
                            Ok(()) => {
                                self.joined_v6.insert(index);
                            }
                            Err(err) => log::debug!("dlna: joining on interface {index}: {err}"),
                        }
                    }
                    let group = SocketAddrV6::new(*GROUP_V6.ip(), GROUP_V6.port(), 0, index);
                    let sent = SockRef::from(socket)
                        .set_multicast_if_v6(index)
                        .and_then(|()| socket.send_to(search.as_bytes(), group));
                    if let Err(err) = sent {
                        log::debug!("dlna: searching on interface {index}: {err}");
                    }
                }
            }
            #[cfg(any(test, feature = "fake"))]
            Network::At(address) => {
                // Straight to it, whatever group the search names.
                let search = ssdp::search(MEDIA_RENDERER, SEARCH_WAIT, GROUP.into());
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

/// `message`, if it came from where it says the device is: a description
/// is fetched only from the address the advert came from, not wherever it
/// points. A device at a link-local IPv6 address is reached through the
/// interface its advert came in on, which its location then says: the
/// zone it gives, if any, is its own.
fn from_its_device(message: Message, from: SocketAddr) -> Option<Message> {
    let Message::Alive(mut advert) = message else {
        return Some(message);
    };
    let mut url = Url::parse(&advert.location)?;
    let address = url.host.split('%').next().unwrap_or_default();
    if address.parse::<IpAddr>().ok()? != from.ip() {
        return None;
    }
    if let SocketAddr::V6(from) = from
        && from.ip().is_unicast_link_local()
    {
        url.host = format!("{}%{}", from.ip(), from.scope_id());
        advert.location = url.to_string();
    }
    Some(Message::Alive(advert))
}

/// Read datagrams off `socket` until `stop`.
fn hear(socket: &UdpSocket, stop: &AtomicBool, inputs: &mpsc::Sender<Input>) {
    let mut buffer = [0; 8 * 1024];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buffer) {
            Ok((n, from)) => {
                if let Some(message) = Message::parse(&buffer[..n]) {
                    log::trace!("dlna: from {from}: {message:?}");
                    let Some(message) = from_its_device(message, from) else {
                        log::debug!("dlna: {from} told of a description elsewhere");
                        continue;
                    };
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
        let (search, search_v6, listen, listen_v6) = match network {
            Network::Multicast => {
                let search = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
                // As UPnP asks, to cross a router or so.
                search.set_multicast_ttl_v4(2)?;
                let search_v6 = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0))
                    .inspect_err(|err| log::info!("dlna: not searching over IPv6: {err}"))
                    .ok();
                let listen = |any: IpAddr| {
                    group_listener(any)
                        .inspect_err(|err| {
                            log::warn!(
                                "dlna: no NOTIFYs to {any}, as SSDP's port can't be had: {err}"
                            );
                        })
                        .ok()
                };
                let listen_v6 = search_v6
                    .as_ref()
                    .and_then(|_| listen(Ipv6Addr::UNSPECIFIED.into()));
                (
                    search,
                    search_v6,
                    listen(Ipv4Addr::UNSPECIFIED.into()),
                    listen_v6,
                )
            }
            // Over the fake's own IP version, from its address.
            #[cfg(any(test, feature = "fake"))]
            Network::At(mut address) => {
                address.set_port(0);
                (
                    UdpSocket::bind(address)?,
                    None,
                    Some(UdpSocket::bind(address)?),
                    None,
                )
            }
        };
        #[cfg(any(test, feature = "fake"))]
        let listening = listen.as_ref().and_then(|listen| listen.local_addr().ok());
        let directory = Arc::new(Mutex::new(Directory::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (inputs, received) = mpsc::channel();
        let (found, receiver) = unbounded();
        let mut threads = Vec::new();
        let sockets = [
            Some(&search),
            search_v6.as_ref(),
            listen.as_ref(),
            listen_v6.as_ref(),
        ];
        for socket in sockets.into_iter().flatten() {
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
            search_v6,
            listen,
            listen_v6,
            joined: HashSet::new(),
            joined_v6: HashSet::new(),
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
