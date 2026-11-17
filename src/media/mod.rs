//! Media playback engine: FFmpeg decoding, a shared audio mixer and clocks.

pub mod audio;
pub mod clock;
pub mod decoder;
mod hwaccel;
pub mod player;
pub mod source;
#[cfg(test)]
mod tests;

pub use audio::{AudioOutput, Mixer};
pub use player::{Player, PlayerOptions};
pub use source::Source;
