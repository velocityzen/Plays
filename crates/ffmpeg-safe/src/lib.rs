//! What ffmpeg-next reads unsoundly from FFmpeg, read soundly. clippy.toml
//! bans the ffmpeg-next calls these stand in for.

mod layout;
mod plane;
mod tag;

pub use layout::Layout;
pub use plane::{bottom_up, plane, plane_mut};
pub use tag::{tag, tags};
