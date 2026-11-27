//! What ffmpeg-next reads unsoundly from FFmpeg, or not at all, read
//! soundly, files opened as themselves, and FFmpeg set up once.
//! clippy.toml bans the ffmpeg-next calls these stand in for.

mod colors;
mod init;
mod input;
mod layout;
mod nal;
mod plane;
mod side_data;
mod stream;
mod subtitle;
mod tag;

pub use colors::{Colors, Tags};
pub use init::init;
pub use input::{FILE_PROTOCOLS, input_file, input_file_with, input_stream, network_options};
pub use layout::Layout;
pub use nal::{extradata, nal_units};
pub use plane::{bottom_up, plane, plane_mut};
pub use side_data::{
    Coded, DisplayMatrix, DolbyVision, Framed, Rpu, coded, frame, has_coded, rpu, sys,
};
pub use stream::{Described, coded_aspect, container_aspect, frame_rate, language};
pub use subtitle::{ass_line, plain_text, subtitle_decoder};
pub use tag::{tag, tags};
