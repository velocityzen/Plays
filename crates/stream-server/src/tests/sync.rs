use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::*;

/// A mutex a thread panicked holding, once it had set it to 1.
fn poisoned() -> Arc<Mutex<u32>> {
    let mutex = Arc::new(Mutex::new(0));
    let held = Arc::clone(&mutex);
    let _ = std::thread::spawn(move || {
        let mut value = held.lock().unwrap();
        *value = 1;
        panic!("holding the lock");
    })
    .join();
    assert!(mutex.is_poisoned());
    mutex
}

#[test]
fn a_lock_goes_on_past_a_panic_with_what_it_left() {
    assert_eq!(*lock(&poisoned()), 1);
}

#[test]
fn a_wait_rung_goes_on_past_a_panic() {
    let mutex = poisoned();
    let changed = Arc::new(Condvar::new());
    let mut guard = lock(&mutex);
    let (held, ringing) = (Arc::clone(&mutex), Arc::clone(&changed));
    std::thread::spawn(move || {
        *lock(&held) = 2;
        ringing.notify_all();
    });
    while *guard != 2 {
        guard = wait(&changed, guard);
    }
}

#[test]
fn a_wait_timed_out_goes_on_past_a_panic() {
    let mutex = poisoned();
    let guard = wait_timeout(&Condvar::new(), lock(&mutex), Duration::from_millis(1));
    assert_eq!(*guard, 1);
}
