//! What ffmpeg-next reads unsoundly from FFmpeg, read soundly. clippy.toml
//! bans the ffmpeg-next calls these stand in for.

mod layout;
mod tag;

pub use layout::Layout;
pub use tag::{tag, tags};
