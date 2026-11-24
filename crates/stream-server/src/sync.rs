//! Locks and waits that go on past a thread that panicked holding the lock:
//! what it left is still served, or fails as a request.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// `mutex`, locked.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `guard`, back once `changed` is rung.
pub(crate) fn wait<'a, T>(changed: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    changed.wait(guard).unwrap_or_else(PoisonError::into_inner)
}

/// `guard`, back once `changed` is rung or `left` has gone by.
pub(crate) fn wait_timeout<'a, T>(
    changed: &Condvar,
    guard: MutexGuard<'a, T>,
    left: Duration,
) -> MutexGuard<'a, T> {
    changed
        .wait_timeout(guard, left)
        .unwrap_or_else(PoisonError::into_inner)
        .0
}

#[cfg(test)]
#[path = "tests/sync.rs"]
mod tests;
