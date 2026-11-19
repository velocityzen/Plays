//! AirPlay receivers: found on the network, paired with, and sent to. A TV
//! plays a URL it's given and is driven while it does; a speaker takes
//! live sound. Everything blocks, on the caller's thread or the session's
//! own; what a session says as it goes comes over a channel.

mod audio;
mod browser;
mod channel;
mod connection;
mod crypto;
mod device;
mod error;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
mod message;
mod pairing;
mod rtp;
mod session;
mod srp;
#[cfg(test)]
mod tests;
mod tlv8;
mod video;

pub use audio::AudioStream;
pub use browser::{Browser, Found};
pub use device::{Device, Features, Pairing, Record};
pub use error::{Error, Result};
pub use pairing::Credentials;
pub use session::{Event, Sender};
pub use video::{PinPairing, PlaybackInfo, PlaybackState, VideoSession};
