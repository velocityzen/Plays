//! What ffmpeg-next reads unsoundly from FFmpeg, read soundly. clippy.toml
//! bans the ffmpeg-next calls these stand in for.

mod layout;

pub use layout::Layout;

#[cfg(test)]
#[path = "tests/layout.rs"]
mod tests;
