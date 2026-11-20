//! What the casting crates share on the network, so a fix in one reaches
//! them all: connecting to the first of a device's addresses that takes
//! it, reading HTTP and RTSP messages within bounds and in time, and, with
//! the `mdns` feature, finding devices over mDNS.

pub mod http;
#[cfg(feature = "mdns")]
pub mod mdns;

use std::io;
use std::net::{SocketAddr, TcpStream};
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

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, TcpListener};

    use super::*;

    const WAIT: Duration = Duration::from_secs(2);

    #[test]
    fn connects_at_the_first_address_that_takes_it() {
        let refused = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let open = listener.local_addr().unwrap();
        let (stream, address) = connect_first(&[refused, open], WAIT).unwrap();
        assert_eq!(address, open);
        assert_eq!(stream.peer_addr().unwrap(), open);
        assert_eq!(connect_first(&[open, refused], WAIT).unwrap().1, open);
    }

    #[test]
    fn with_none_taking_it_the_last_refusal_is_the_error() {
        let refused = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap();
        let err = connect_first(&[refused], WAIT).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused);
        let err = connect_first(&[], WAIT).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_lock_a_panic_left_poisoned_still_locks() {
        let mutex = Mutex::new(1);
        let _ = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _held = mutex.lock().unwrap();
                    panic!("while holding it");
                })
                .join()
        });
        assert!(mutex.is_poisoned());
        *lock(&mutex) += 1;
        assert_eq!(*lock(&mutex), 2);
    }
}
