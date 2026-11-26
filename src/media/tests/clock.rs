use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::*;

#[test]
fn the_mixer_never_waits_for_the_clock() {
    let clock = Arc::new(Clock::new());
    clock.start();
    let held = clock.inner.lock();
    let nudging = thread::spawn({
        let clock = clock.clone();
        move || {
            let start = Instant::now();
            clock.sync(5.0, Instant::now());
            let gone = clock.try_now();
            (start.elapsed(), gone)
        }
    });
    let hold = Duration::from_millis(200);
    thread::sleep(hold);
    drop(held);
    let (took, now) = nudging.join().unwrap();
    assert!(took < hold / 4, "waited for the clock");
    assert_eq!(now, None, "no time to tell while it's held");
    assert!(clock.now() < 1.0, "the nudge it skipped");
    clock.sync(5.0, Instant::now());
    assert!((clock.now() - 5.0).abs() < 0.01, "and the next one");
}

#[test]
fn a_clock_at_a_rate_runs_that_many_media_seconds_a_second() {
    let clock = Clock::new();
    clock.set(10.0);
    clock.start();
    clock.set_rate(2.0);
    let inner = clock.inner.lock();
    let anchor = inner.anchor.expect("it runs on");
    assert!((inner.base - 10.0).abs() < 0.01, "it keeps its time");
    let later = inner.at(anchor, anchor + Duration::from_secs(3));
    assert!((later - (inner.base + 6.0)).abs() < 1e-9, "at {later}");
}

#[test]
fn a_rate_set_while_paused_is_kept_for_when_it_runs() {
    let clock = Clock::new();
    clock.set(4.0);
    clock.set_rate(0.5);
    assert_eq!(clock.now(), 4.0);
    assert!(clock.inner.lock().anchor.is_none(), "still paused");
    clock.start();
    let inner = clock.inner.lock();
    let anchor = inner.anchor.unwrap();
    let later = inner.at(anchor, anchor + Duration::from_secs(2));
    assert!((later - 5.0).abs() < 1e-9, "at {later}");
}

#[test]
fn the_mixer_nudges_a_clock_at_a_rate_only_off_that_rate() {
    let clock = Clock::new();
    clock.start();
    clock.set_rate(2.0);
    let (anchor, base) = {
        let inner = clock.inner.lock();
        (inner.anchor.unwrap(), inner.base)
    };
    // Heard a second on where twice the speed has it: nothing to nudge.
    let at = anchor + Duration::from_secs(1);
    clock.sync(base + 2.0, at);
    let inner = clock.inner.lock();
    assert!((inner.base - (base + 2.0)).abs() < 1e-9);
    assert_eq!(inner.anchor, Some(at));
}
