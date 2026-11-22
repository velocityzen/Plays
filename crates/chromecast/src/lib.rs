//! Google Cast devices: Chromecasts, Google TVs, Nest speakers and speaker
//! groups, found on the network and told to play a URL, then driven while
//! they do. Everything blocks, on the caller's thread or a session's own;
//! what a session says as it goes comes over a channel.

mod browser;
mod channel;
mod device;
mod error;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
mod media;
mod proto;
mod session;
#[cfg(test)]
#[path = "tests/chromecast.rs"]
mod tests;

pub use browser::{Browser, Found};
pub use device::{Capabilities, Device, Record};
pub use error::{Error, Result};
pub use media::{
    Color, Edge, FontFamily, HlsSegments, IdleReason, Media, MediaStatus, PlayerState, StreamType,
    TextTrack, TextTrackStyle, Volume,
};
pub use session::{DEFAULT_MEDIA_RECEIVER, Ended, Event, Load, Session};
