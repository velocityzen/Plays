//! An encrypted connection, once pairing has given a secret: each frame is
//! two bytes of length, little-endian, then up to 1024 bytes sealed with
//! the length as associated data. Each direction has its key, and counts
//! its frames for their nonces.

use std::io::{self, Read, Write};

use crate::crypto::{self, counter_nonce};

/// Most plaintext a frame carries.
const FRAME: usize = 1024;
const TAG: usize = 16;

/// A channel's keys, from our side.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Keys {
    pub write: [u8; 32],
    pub read: [u8; 32],
}

impl Keys {
    /// The control connection's: RTSP and HTTP to the receiver.
    pub fn control(secret: &[u8]) -> Self {
        Self {
            write: crypto::derive(secret, "Control-Salt", "Control-Write-Encryption-Key"),
            read: crypto::derive(secret, "Control-Salt", "Control-Read-Encryption-Key"),
        }
    }

    /// The event connection's, which the receiver writes on: named from
    /// its side, so ours are the other way round.
    pub fn events(secret: &[u8]) -> Self {
        Self {
            write: crypto::derive(secret, "Events-Salt", "Events-Read-Encryption-Key"),
            read: crypto::derive(secret, "Events-Salt", "Events-Write-Encryption-Key"),
        }
    }

    /// The other side's.
    #[cfg(any(test, feature = "fake"))]
    pub fn swapped(self) -> Self {
        Self {
            write: self.read,
            read: self.write,
        }
    }
}

struct Direction {
    key: [u8; 32],
    frames: u64,
}

impl Direction {
    fn next(&mut self) -> [u8; 8] {
        let nonce = counter_nonce(self.frames);
        self.frames += 1;
        nonce
    }
}

pub struct Sealed<S> {
    stream: S,
    write: Direction,
    read: Direction,
    /// Opened but not yet read.
    plain: Vec<u8>,
    at: usize,
}

impl<S> Sealed<S> {
    pub fn new(stream: S, keys: Keys) -> Self {
        Self {
            stream,
            write: Direction {
                key: keys.write,
                frames: 0,
            },
            read: Direction {
                key: keys.read,
                frames: 0,
            },
            plain: Vec::new(),
            at: 0,
        }
    }

    pub fn get_ref(&self) -> &S {
        &self.stream
    }

    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream
    }
}

impl<S: Read> Sealed<S> {
    /// The next frame's plaintext; empty at the end of the stream.
    fn frame(&mut self) -> io::Result<Vec<u8>> {
        let mut length = [0; 2];
        match self.stream.read(&mut length[..1])? {
            0 => return Ok(Vec::new()),
            _ => self.stream.read_exact(&mut length[1..])?,
        }
        let mut sealed = vec![0; usize::from(u16::from_le_bytes(length)) + TAG];
        self.stream.read_exact(&mut sealed)?;
        let nonce = self.read.next();
        crypto::open(&self.read.key, nonce, &length, &sealed)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
    }
}

impl<S: Read> Read for Sealed<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.at == self.plain.len() {
            let frame = self.frame()?;
            if frame.is_empty() {
                return Ok(0);
            }
            self.plain = frame;
            self.at = 0;
        }
        let n = buf.len().min(self.plain.len() - self.at);
        buf[..n].copy_from_slice(&self.plain[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

impl<S: Write> Write for Sealed<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut out = Vec::with_capacity(buf.len() + buf.len().div_ceil(FRAME) * (2 + TAG));
        for chunk in buf.chunks(FRAME) {
            let length = (chunk.len() as u16).to_le_bytes();
            let nonce = self.write.next();
            out.extend_from_slice(&length);
            out.extend(crypto::seal(&self.write.key, nonce, &length, chunk));
        }
        self.stream.write_all(&out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[cfg(test)]
#[path = "tests/channel.rs"]
mod tests;
