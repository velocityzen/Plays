use std::time::Instant;

use parking_lot::Mutex;

/// Playback clock for a single player.
///
/// It runs on the system clock between updates, and the audio mixer nudges it
/// toward the position of the samples that are actually being heard. That makes
/// audio the master when a file has sound and keeps video-only files on wall time.
pub struct Clock {
    inner: Mutex<Inner>,
}

struct Inner {
    /// Media time (seconds) at `anchor`.
    base: f64,
    /// When the clock is running, the instant `base` was recorded at.
    anchor: Option<Instant>,
}

impl Inner {
    fn now(&self) -> f64 {
        match self.anchor {
            Some(anchor) => self.base + anchor.elapsed().as_secs_f64(),
            None => self.base,
        }
    }
}

/// Drift larger than this snaps the clock instead of easing it.
const SNAP_THRESHOLD: f64 = 0.08;
/// Fraction of the drift that is corrected on every audio callback.
const EASE: f64 = 0.15;

impl Clock {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                base: 0.0,
                anchor: None,
            }),
        }
    }

    pub fn now(&self) -> f64 {
        self.inner.lock().now()
    }

    /// `now`, unless another thread has the clock this moment: for the
    /// audio mixer, which mustn't wait.
    pub fn try_now(&self) -> Option<f64> {
        self.inner.try_lock().map(|inner| inner.now())
    }

    pub fn start(&self) {
        let mut inner = self.inner.lock();
        if inner.anchor.is_none() {
            inner.anchor = Some(Instant::now());
        }
    }

    pub fn pause(&self) {
        let mut inner = self.inner.lock();
        inner.base = inner.now();
        inner.anchor = None;
    }

    pub fn set(&self, time: f64) {
        let mut inner = self.inner.lock();
        inner.base = time;
        if inner.anchor.is_some() {
            inner.anchor = Some(Instant::now());
        }
    }

    /// Ease toward `time`, the media time at `at` of sound heard
    /// elsewhere, however far it is: a device's clock jitters more than
    /// the mixer's, so it is never snapped to.
    pub fn ease_to(&self, time: f64, at: Instant) {
        let mut inner = self.inner.lock();
        let Some(anchor) = inner.anchor else {
            return;
        };
        let current = inner.base + at.saturating_duration_since(anchor).as_secs_f64();
        inner.base = current + (time - current) * EASE;
        inner.anchor = Some(at);
    }

    /// Called by the audio mixer: `time` is the media time being heard at
    /// `at`. Not when another thread has the clock this moment: the mixer
    /// mustn't wait, and its next call nudges it.
    pub fn sync(&self, time: f64, at: Instant) {
        let Some(mut inner) = self.inner.try_lock() else {
            return;
        };
        let Some(anchor) = inner.anchor else {
            return;
        };
        let current = inner.base + at.saturating_duration_since(anchor).as_secs_f64();
        let drift = time - current;
        inner.base = if drift.abs() > SNAP_THRESHOLD {
            time
        } else {
            current + drift * EASE
        };
        inner.anchor = Some(at);
    }
}

#[cfg(test)]
mod tests {
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
}
