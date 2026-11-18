//! Media playback engine: FFmpeg decoding, a shared audio mixer and clocks.

pub mod audio;
pub mod clock;
pub mod decoder;
pub mod dovi;
pub mod hdr;
mod hwaccel;
pub mod picture;
pub mod player;
pub mod source;
pub mod subtitles;
#[cfg(test)]
mod tests;
pub mod tracks;
#[cfg(target_os = "macos")]
mod videotoolbox;

pub use audio::{AudioOutput, Mixer};
pub use picture::Picture;
pub use player::{Player, PlayerOptions};
pub use source::Source;
