//! UPnP media renderers, as DLNA has them: smart TVs and speakers found on
//! the network, told to play a URL, then driven while they do. Everything
//! blocks, on the caller's thread or a session's own; what a browser finds
//! and what a session says as it goes come over channels.

#![forbid(unsafe_code)]

mod browser;
mod device;
mod error;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
mod http;
mod media;
mod session;
mod soap;
mod ssdp;
#[cfg(test)]
#[path = "tests/dlna.rs"]
mod tests;
mod xml;

pub use browser::{Browser, Found};
pub use device::{Device, Icon, Service};
pub use error::{Error, Result};
pub use media::{Capability, Kind, Media, Position, Status, Subtitles, TransportState, Volume};
pub use session::{Ended, Event, Session};
pub use soap::Fault;
