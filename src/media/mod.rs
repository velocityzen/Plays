//! Media playback engine: FFmpeg decoding, a shared audio mixer and clocks.

pub mod audio;
pub mod clock;
pub mod decoder;
pub mod details;
pub mod dovi;
mod external;
pub mod hdr;
mod hwaccel;
pub mod orientation;
pub mod output;
pub mod picture;
pub mod player;
pub mod source;
pub mod still;
pub mod subtitles;
#[cfg(test)]
mod tests;
pub mod tracks;
#[cfg(target_os = "macos")]
mod videotoolbox;
// Its math is tested on macOS too, where frames are surfaces instead.
#[cfg(any(not(target_os = "macos"), test))]
pub mod yuv;

pub use audio::Mixer;
pub use orientation::Orientation;
pub use output::AudioOutput;
pub use picture::Picture;
pub use player::{Player, PlayerOptions};
pub use source::{Neighbors, Source};
