//! A file on disk, opened as itself. FFmpeg follows what a file names, a
//! playlist's segments and keys, over any protocol it has: a file opened
//! without a whitelist fetches from wherever it says.

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

#[cfg(test)]
#[path = "tests/input.rs"]
mod tests;
