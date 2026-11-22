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
