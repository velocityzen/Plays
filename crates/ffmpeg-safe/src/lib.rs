//! What ffmpeg-next reads unsoundly from FFmpeg, read soundly, and files
//! opened as themselves. clippy.toml bans the ffmpeg-next calls these
//! stand in for.

mod input;
mod layout;
mod plane;
mod subtitle;
mod tag;

pub use input::{FILE_PROTOCOLS, input_file, input_file_with};
pub use layout::Layout;
pub use plane::{bottom_up, plane, plane_mut};
pub use subtitle::{ass_line, plain_text};
pub use tag::{tag, tags};
