//! A file on disk, opened as itself, and a stream on the network, opened
//! as Plays waits on one. FFmpeg follows what a file names, a playlist's
//! segments and keys, over any protocol it has: a file opened without a
//! whitelist fetches from wherever it says.

use std::path::Path;

use ffmpeg_next::format::context::Input;
use ffmpeg_next::{Dictionary, Error, format};

/// What a file on disk may reach: files, the decryption of an HLS
/// segment's, and `data:` URIs.
pub const FILE_PROTOCOLS: &str = "file,crypto,data";

/// Open the file at `path`, reaching only `FILE_PROTOCOLS`.
pub fn input_file<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Input, Error> {
    input_file_with(path, || false, Dictionary::new())
}

/// Open the file at `path` with `options`, reaching only
/// `FILE_PROTOCOLS`, giving up once `interrupt` says to.
pub fn input_file_with<P: AsRef<Path> + ?Sized>(
    path: &P,
    interrupt: impl FnMut() -> bool + Send + 'static,
    mut options: Dictionary<'_>,
) -> Result<Input, Error> {
    options.set("protocol_whitelist", FILE_PROTOCOLS);
    format::input_with_interrupt_and_dictionary(path, interrupt, options)
}

/// Network reads and connections give up after this long without data.
const NETWORK_TIMEOUT_MICROS: &str = "15000000";

/// What a stream on the network is opened with: reads and connections give
/// up after 15 seconds without data, a dropped connection is made again,
/// and RTSP comes over TCP, as most players have it, since UDP rarely makes
/// it through NAT and firewalls.
pub fn network_options() -> Dictionary<'static> {
    let mut options = Dictionary::new();
    options.set("rw_timeout", NETWORK_TIMEOUT_MICROS);
    options.set("timeout", NETWORK_TIMEOUT_MICROS);
    options.set("reconnect", "1");
    options.set("reconnect_streamed", "1");
    options.set("reconnect_delay_max", "5");
    options.set("rtsp_transport", "tcp");
    options
}

/// Open the stream at `url` with `network_options`, giving up once
/// `interrupt` says to.
pub fn input_stream<P: AsRef<Path> + ?Sized>(
    url: &P,
    interrupt: impl FnMut() -> bool + Send + 'static,
) -> Result<Input, Error> {
    format::input_with_interrupt_and_dictionary(url, interrupt, network_options())
}

#[cfg(test)]
#[path = "tests/input.rs"]
mod tests;
