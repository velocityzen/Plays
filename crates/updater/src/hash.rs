//! SHA-256, as the feed and update files write it: lowercase hex.

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

/// `hex`'s 32 bytes, if it's 64 hex digits.
pub fn parse(hex: &str) -> Option<[u8; 32]> {
    let digits = hex.as_bytes();
    if digits.len() != 64 {
        return None;
    }
    let mut bytes = [0; 32];
    for (byte, pair) in bytes.iter_mut().zip(digits.chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(bytes)
}

/// `bytes`' SHA-256, in lowercase hex.
pub fn of(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The file at `path`'s, read through.
pub fn of_file(path: &Path) -> io::Result<String> {
    let mut hashing = Hashing::new(io::sink());
    io::copy(&mut fs::File::open(path)?, &mut hashing)?;
    Ok(hashing.finish().0)
}

/// Whether `actual` is `expected`, which may be in either case.
pub fn same(actual: &str, expected: &str) -> bool {
    parse(actual).is_some() && parse(actual) == parse(expected)
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A writer that hashes and counts what goes through it.
pub struct Hashing<W> {
    inner: W,
    hasher: Sha256,
    written: u64,
}

impl<W: Write> Hashing<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            written: 0,
        }
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    /// What went through: its SHA-256, and the writer.
    pub fn finish(self) -> (String, W) {
        (hex(&self.hasher.finalize()), self.inner)
    }
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Copy `from` into `to`, stopping with an error once more than `limit`
/// bytes have come.
pub fn copy_at_most(from: &mut impl Read, to: &mut impl Write, limit: u64) -> io::Result<u64> {
    let copied = io::copy(&mut from.take(limit + 1), to)?;
    if copied > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("more than {limit} bytes"),
        ));
    }
    Ok(copied)
}
