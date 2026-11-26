use std::time::Instant;

use parking_lot::Mutex;

/// Playback clock for a single player.
///
/// It runs on the system clock between updates, at the player's speed, and
/// the audio mixer nudges it toward the position of the samples that are
/// actually being heard. That makes audio the master when a file has sound
/// and keeps video-only files on wall time.
pub struct Clock {
    inner: Mutex<Inner>,
}

struct Inner {
    /// Media time (seconds) at `anchor`.
    base: f64,
    /// When the clock is running, the instant `base` was recorded at.
    anchor: Option<Instant>,
    /// Media seconds a second: 1 at the media's own speed.
    rate: f64,
}

impl Inner {
    fn now(&self) -> f64 {
        match self.anchor {
            Some(anchor) => self.at(anchor, Instant::now()),
            None => self.base,
        }
    }

    /// Media time at `at`, running from `anchor`.
    fn at(&self, anchor: Instant, at: Instant) -> f64 {
        self.base + at.saturating_duration_since(anchor).as_secs_f64() * self.rate
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
                rate: 1.0,
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

    /// Run at `rate` media seconds a second from now on.
    pub fn set_rate(&self, rate: f64) {
        let mut inner = self.inner.lock();
        inner.base = inner.now();
        if inner.anchor.is_some() {
            inner.anchor = Some(Instant::now());
        }
        inner.rate = rate;
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
        let current = inner.at(anchor, at);
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
        let current = inner.at(anchor, at);
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
#[path = "tests/clock.rs"]
mod tests;
