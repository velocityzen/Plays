//! FFmpeg set up once in a process, however many parts of it ask.

use std::sync::OnceLock;

use ffmpeg_next::{Error, ffi};

/// FFmpeg set up, once in a process however often asked: its errors named,
/// and its network, TLS's libraries too, before anything opens an https://
/// stream. How much its log says is each caller's own to set.
pub fn init() -> Result<(), Error> {
    static DONE: OnceLock<Result<(), Error>> = OnceLock::new();
    *DONE.get_or_init(|| {
        ffmpeg_next::init()?;
        // SAFETY: once in the process, before anything opens a stream, as
        // whatever opens one asks for this first.
        unsafe { ffi::avformat_network_init() };
        Ok(())
    })
}

#[cfg(test)]
#[path = "tests/init.rs"]
mod tests;
