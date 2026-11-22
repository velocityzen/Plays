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
