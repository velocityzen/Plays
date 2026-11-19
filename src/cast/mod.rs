//! Casting a tile to a TV or a speaker. Each protocol is a `Target`, which
//! finds devices and connects to one for a `Session` that plays what the
//! stream server serves. `Casting` keeps the targets, the devices they
//! have found, and the stream server, started for the first cast; a
//! tile's `Link` drives its device on a thread of its own.

#[cfg(test)]
pub mod fake;
mod link;
pub mod target;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Global};
use parking_lot::Mutex;
use stream_server::Server;

pub use link::{Link, Phase, Request, SubtitleChoice};
pub use target::{Device, Error, Shows, Target};

/// How often the targets are asked what they have found.
const DEVICES_POLL: Duration = Duration::from_secs(1);

pub struct Casting {
    targets: Vec<Arc<dyn Target>>,
    /// What the targets had found when last asked, by name.
    devices: Vec<Device>,
    /// Where the stream server listens.
    address: SocketAddr,
    server: Mutex<Option<Arc<Server>>>,
}

impl Global for Casting {}

impl Casting {
    /// The stream server listens on every interface, as devices are
    /// elsewhere on the network.
    pub fn new(targets: Vec<Arc<dyn Target>>) -> Self {
        Self::serving_on(
            targets,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        )
    }

    /// For tests: serving this computer only, where fake devices are, so
    /// no firewall asks about it.
    #[cfg(test)]
    pub fn local(targets: Vec<Arc<dyn Target>>) -> Self {
        Self::serving_on(targets, SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
    }

    fn serving_on(targets: Vec<Arc<dyn Target>>, address: SocketAddr) -> Self {
        let mut casting = Self {
            targets,
            devices: Vec::new(),
            address,
            server: Mutex::default(),
        };
        casting.devices = casting.found();
        casting
    }

    /// The devices found, by name.
    pub fn devices(cx: &App) -> &[Device] {
        cx.try_global::<Self>()
            .map_or(&[], |casting| casting.devices.as_slice())
    }

    /// The devices a tile can cast to: those that show video, and for a
    /// tile that plays only sound, speakers too.
    pub fn devices_for(cx: &App, has_video: bool) -> Vec<Device> {
        Self::devices(cx)
            .iter()
            .filter(|device| device.shows == Shows::Video || !has_video)
            .cloned()
            .collect()
    }

    fn found(&self) -> Vec<Device> {
        let mut devices: Vec<Device> = self
            .targets
            .iter()
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

    /// Keep the devices found up to date.
    pub fn watch(cx: &mut App) {
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
            .targets
            .iter()
            .find(|target| target.kind() == device.kind)
            .cloned()
            .ok_or_else(|| format!("{} devices can't be reached", device.kind))?;
        let server = casting.server()?;
        Ok(Link::start(target, device.clone(), server, request))
    }

    /// The stream server, started the first time it's needed.
    fn server(&self) -> Result<Arc<Server>, String> {
        let mut server = self.server.lock();
        if let Some(server) = &*server {
            return Ok(Arc::clone(server));
        }
        let started = Arc::new(
            Server::start_on(self.address)
                .map_err(|err| format!("can't serve the media: {err}"))?,
        );
        log::info!("stream server on port {}", started.port());
        *server = Some(Arc::clone(&started));
        Ok(started)
    }
}
