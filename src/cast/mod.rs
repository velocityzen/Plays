//! Casting a tile to a TV or a speaker. Each protocol is a `Target`, which
//! finds devices and connects to one for a `Session` that plays what the
//! stream server serves. `Casting` keeps the protocols, looking with those
//! Settings have on, the devices they have found, and the stream server,
//! started for the first cast; a tile's `Link` drives its device on a
//! thread of its own.

pub mod chromecast;
pub mod dlna;
#[cfg(test)]
pub mod fake;
mod link;
pub mod target;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gpui::{App, BorrowAppContext, Global};
use parking_lot::Mutex;
use stream_server::Server;

pub use link::{Link, Phase, Request, SubtitleChoice};
pub use target::{Device, Error, Shows, Target};

use crate::settings::Settings;

/// The threads stopping devices cast to from windows already closed, for
/// the app to wait for as it quits, as closing the last window quits it.
#[derive(Default)]
pub struct Stopping(Vec<JoinHandle<()>>);

impl Global for Stopping {}

impl Stopping {
    pub fn add(threads: Vec<JoinHandle<()>>, cx: &mut App) {
        let stopping = cx.default_global::<Self>();
        stopping.0.retain(|thread| !thread.is_finished());
        stopping.0.extend(threads);
    }

    pub fn take(cx: &mut App) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut cx.default_global::<Self>().0)
    }
}

/// How often the targets are asked what they have found.
const DEVICES_POLL: Duration = Duration::from_secs(1);

/// Let go of a target that has stopped looking, on a thread of its own: it
/// waits for its threads to end as it goes, which can take a second, and
/// the windows would stop drawing meanwhile. A cast to one of its devices
/// keeps it until the cast ends.
fn let_go(target: Arc<dyn Target>) {
    let spawned = thread::Builder::new()
        .name("cast stop looking".into())
        .spawn(move || drop(target));
    if let Err(err) = spawned {
        log::warn!("cast: no thread to stop looking on, so it stops here: {err}");
    }
}

/// A protocol Plays casts with: its name, what it finds, and how it starts
/// looking for them.
pub struct Protocol {
    /// Its devices' `kind`, and what Settings call it.
    pub kind: &'static str,
    /// What it finds, as Settings say.
    pub finds: &'static str,
    start: Box<dyn Fn() -> Result<Arc<dyn Target>, String>>,
}

impl Protocol {
    pub fn new(
        kind: &'static str,
        finds: &'static str,
        start: impl Fn() -> Result<Arc<dyn Target>, String> + 'static,
    ) -> Self {
        Self {
            kind,
            finds,
            start: Box::new(start),
        }
    }
}

/// The protocols Plays casts with.
pub fn protocols() -> Vec<Protocol> {
    vec![
        Protocol::new(
            chromecast::KIND,
            "Chromecasts, Google TVs and Nest speakers",
            || {
                let target = chromecast::ChromecastTarget::start()
                    .map_err(|err| format!("can't look for Cast devices: {err}"))?;
                Ok(Arc::new(target))
            },
        ),
        Protocol::new(
            dlna::KIND,
            "TVs and speakers that play from the network",
            || {
                let target = dlna::DlnaTarget::start()
                    .map_err(|err| format!("can't look for DLNA renderers: {err}"))?;
                Ok(Arc::new(target))
            },
        ),
    ]
}

/// Whether a protocol is looking for devices.
#[derive(Clone, PartialEq, Debug)]
pub enum Looking {
    /// Turned off in Settings.
    Off,
    On,
    /// On, but it couldn't start, for this reason.
    Failed(String),
}

/// A protocol, and its target while it's on: looking, or why it can't.
struct Kept {
    protocol: Protocol,
    target: Option<Result<Arc<dyn Target>, String>>,
}

impl Kept {
    fn looking(&self) -> Looking {
        match &self.target {
            None => Looking::Off,
            Some(Ok(_)) => Looking::On,
            Some(Err(why)) => Looking::Failed(why.clone()),
        }
    }

    fn target(&self) -> Option<&Arc<dyn Target>> {
        self.target.as_ref()?.as_ref().ok()
    }
}

pub struct Casting {
    protocols: Vec<Kept>,
    /// What the targets had found when last asked, by name.
    devices: Vec<Device>,
    /// Where the stream server listens.
    address: SocketAddr,
    /// The stream server while a cast uses it: it stops once none does.
    server: Mutex<Weak<Server>>,
}

impl Global for Casting {}

impl Casting {
    /// None of `protocols` looking yet, until `look_as_set`. The stream
    /// server listens on every interface, as devices are elsewhere on the
    /// network.
    pub fn new(protocols: Vec<Protocol>) -> Self {
        let kept = protocols
            .into_iter()
            .map(|protocol| Kept {
                protocol,
                target: None,
            })
            .collect();
        Self::serving_on(kept, SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
    }

    /// For tests: these targets, looking already, each a protocol of its
    /// own which starts it again when turned on; serving this computer
    /// only, where fake devices are, so no firewall asks about it.
    #[cfg(test)]
    pub fn local(targets: Vec<Arc<dyn Target>>) -> Self {
        let kept = targets
            .into_iter()
            .map(|target| {
                let again = Arc::clone(&target);
                Kept {
                    protocol: Protocol::new(target.kind(), "fake devices", move || {
                        Ok(Arc::clone(&again))
                    }),
                    target: Some(Ok(target)),
                }
            })
            .collect();
        Self::serving_on(kept, SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
    }

    fn serving_on(protocols: Vec<Kept>, address: SocketAddr) -> Self {
        let mut casting = Self {
            protocols,
            devices: Vec::new(),
            address,
            server: Mutex::default(),
        };
        casting.devices = casting.found();
        casting
    }

    /// Start the protocols Settings have on looking, and stop those off,
    /// their devices no longer found, away from the windows (`let_go`). A
    /// cast to one goes on; one that couldn't start tries again when turned
    /// off and on.
    pub fn look_as_set(cx: &mut App) {
        let set = &cx.global::<Settings>().casting;
        let Some(casting) = cx.try_global::<Self>() else {
            return;
        };
        if casting
            .protocols
            .iter()
            .all(|kept| kept.target.is_some() == set.looks_with(kept.protocol.kind))
        {
            return;
        }
        let set = set.clone();
        cx.update_global::<Self, _>(|casting, _| {
            for kept in &mut casting.protocols {
                let on = set.looks_with(kept.protocol.kind);
                if !on {
                    if let Some(Ok(target)) = kept.target.take() {
                        let_go(target);
                    }
                } else if kept.target.is_none() {
                    let started = (kept.protocol.start)();
                    if let Err(why) = &started {
                        log::warn!("{why}");
                    }
                    kept.target = Some(started);
                }
            }
            casting.devices = casting.found();
        });
    }

    /// Each protocol, and whether it's looking.
    pub fn protocols(cx: &App) -> Vec<(&'static str, &'static str, Looking)> {
        cx.try_global::<Self>().map_or_else(Vec::new, |casting| {
            casting
                .protocols
                .iter()
                .map(|kept| (kept.protocol.kind, kept.protocol.finds, kept.looking()))
                .collect()
        })
    }

    /// The devices found, by name.
    pub fn devices(cx: &App) -> &[Device] {
        cx.try_global::<Self>()
            .map_or(&[], |casting| casting.devices.as_slice())
    }

    /// The devices a tile can cast to: those that show video, and for a
    /// tile with sound, speakers too, which a video sends its sound to.
    pub fn devices_for(cx: &App, has_audio: bool) -> Vec<Device> {
        Self::devices(cx)
            .iter()
            .filter(|device| device.shows == Shows::Video || has_audio)
            .cloned()
            .collect()
    }

    fn found(&self) -> Vec<Device> {
        let mut devices: Vec<Device> = self
            .protocols
            .iter()
            .filter_map(Kept::target)
            .flat_map(|target| target.devices())
            .collect();
        devices.sort_by_cached_key(|device| (device.name.to_lowercase(), device.kind));
        devices
    }

    /// Ask the targets what they have found now; observers hear of a
    /// change.
    pub fn refresh(cx: &mut App) {
        let Some(casting) = cx.try_global::<Self>() else {
            return;
        };
        let found = casting.found();
        if found != casting.devices {
            cx.global_mut::<Self>().devices = found;
        }
    }

    /// Look with the protocols Settings have on, as they change, and keep
    /// the devices found up to date.
    pub fn watch(cx: &mut App) {
        Self::look_as_set(cx);
        cx.observe_global::<Settings>(Self::look_as_set).detach();
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(DEVICES_POLL).await;
                if cx.update(Self::refresh).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Start casting `request` to `device`: connecting, serving and
    /// loading go on on the link's thread.
    pub fn link(cx: &App, device: &Device, request: Request) -> Result<Link, String> {
        let casting = cx
            .try_global::<Self>()
            .ok_or_else(|| "casting isn't set up".to_owned())?;
        let target = casting
            .protocols
            .iter()
            .filter_map(Kept::target)
            .find(|target| target.kind() == device.kind)
            .cloned()
            .ok_or_else(|| format!("{} devices can't be reached", device.kind))?;
        let server = casting.server()?;
        Ok(Link::start(target, device.clone(), server, request))
    }

    /// The stream server, started when a cast needs it and none is
    /// running: casts hold it, and it stops listening once the last ends.
    fn server(&self) -> Result<Arc<Server>, String> {
        let mut server = self.server.lock();
        if let Some(server) = server.upgrade() {
            return Ok(server);
        }
        let started = Arc::new(
            Server::start_on(self.address)
                .map_err(|err| format!("can't serve the media: {err}"))?,
        );
        log::info!("stream server on port {}", started.port());
        *server = Arc::downgrade(&started);
        Ok(started)
    }
}
