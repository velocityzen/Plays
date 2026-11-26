//! What the casting crates share on the network, so a fix in one reaches
//! them all: connecting to the first of a device's addresses that takes
//! it, connections taken that close for the other end however they're let
//! go of (`Accepted`), reading HTTP and RTSP messages within bounds and in
//! time, and, with the `mdns` feature, finding devices over mDNS.

#![forbid(unsafe_code)]

pub mod http;
#[cfg(feature = "mdns")]
pub mod mdns;

use std::io;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// A change in what's found.
#[derive(Clone, PartialEq, Debug)]
pub enum Found<D> {
    Added(D),
    /// Its name, address, or what it says of itself changed.
    Changed(D),
    Removed(D),
}

/// Lock `mutex`, even if a thread panicked holding it: what it guards is
/// still whole, as nothing here leaves it halfway changed.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Connect to the first of `addresses` that takes it within `timeout`, in
/// order: a device on more than one network has an address on each, and
/// not all reach it from here. Gives the address it was.
pub fn connect_first(
    addresses: &[SocketAddr],
    timeout: Duration,
) -> io::Result<(TcpStream, SocketAddr)> {
    let mut refused = None;
    for &address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => return Ok((stream, address)),
            Err(err) => {
                log::debug!("{address}: {err}");
                refused = Some(err);
            }
        }
    }
    Err(refused.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address")))
}

/// A connection a listener took. Dropped, it's shut down both ways, not
/// only closed here: on macOS a connection is taken, then kept from
/// processes started after, so one started between the two has it too,
/// and would keep it open, the other end waiting on it.
#[derive(Debug)]
pub struct Accepted(TcpStream);

impl From<TcpStream> for Accepted {
    fn from(stream: TcpStream) -> Self {
        Self(stream)
    }
}

impl Deref for Accepted {
    type Target = TcpStream;

    fn deref(&self) -> &TcpStream {
        &self.0
    }
}

impl DerefMut for Accepted {
    fn deref_mut(&mut self) -> &mut TcpStream {
        &mut self.0
    }
}

impl Drop for Accepted {
    fn drop(&mut self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
#[path = "tests/network.rs"]
mod tests;
