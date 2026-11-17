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

    /// Called by the audio mixer: `time` is the media time being heard at `at`.
    pub fn sync(&self, time: f64, at: Instant) {
        let mut inner = self.inner.lock();
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
