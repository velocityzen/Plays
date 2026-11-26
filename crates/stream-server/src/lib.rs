//! Serving media to TVs and speakers over HTTP. A protocol's crate tells a
//! device to play a URL; this crate makes that URL: the file as it is when
//! the device plays it, else HLS (or, for DLNA TVs, an MPEG-TS stream) with
//! each track copied when the device decodes it and re-encoded when it
//! doesn't. Video is re-encoded on the computer's hardware encoder only.
//! Subtitles are served beside it, as WebVTT and SRT.
//!
//! ```no_run
//! use stream_server::{CastRequest, Profile, Server, Source};
//!
//! let server = Server::start()?;
//! let cast = server.add(
//!     CastRequest::new(Source::File("movie.mkv".into()), Profile::default()),
//!     "192.168.1.20:0".parse().unwrap(),
//! )?;
//! println!("tell the TV to play {} ({})", cast.url(), cast.mime());
//! # Ok::<(), stream_server::Error>(())
//! ```

mod adjust;
mod encode;
mod hdr;
mod http;
mod job;
mod media;
mod mux;
mod overlay;
mod plan;
mod playlist;
mod profile;
mod segments;
mod server;
mod subtitles;
mod sync;

#[cfg(test)]
#[path = "tests/stream_server.rs"]
mod tests;

use std::fmt;

use ffmpeg_next as ffmpeg;

pub use adjust::Adjustments;
pub use media::{AudioInfo, MediaInfo, Source, StreamInfo, StreamKind, VideoInfo};
pub use plan::{AudioPlan, Delivery, Pick, Plan, VideoPlan};
pub use profile::{AudioCodec, Container, Hdr, Profile, Segments, VideoCodec};
pub use server::{Cast, CastRequest, Server};
pub use subtitles::SubtitleSource;

#[derive(Debug)]
pub enum Error {
    /// The media couldn't be opened or read.
    Source(String),
    /// The device can't play it, however it is served.
    Unsupported(String),
    /// The device needs video re-encoded, and this computer has no hardware
    /// encoder for a codec it decodes.
    NoEncoder,
    Encode(String),
    Mux(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(message) | Self::Encode(message) | Self::Mux(message) => {
                f.write_str(message)
            }
            Self::Unsupported(why) => write!(f, "the device can't play this: {why}"),
            Self::NoEncoder => f.write_str(
                "the device needs the video re-encoded, and this computer has no hardware \
                 encoder for a codec it plays",
            ),
            Self::Io(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// FFmpeg's set-up, once, before anything of it is used.
fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if let Err(err) = ffmpeg::init() {
            log::warn!("FFmpeg didn't initialize: {err}");
        }
    });
}
