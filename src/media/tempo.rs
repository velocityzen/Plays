//! Sound played faster or slower at the same pitch: WSOLA, waveform
//! similarity overlap-add. Each window of what's heard is taken from about
//! where the speed says in the sound, nudged to where it goes on best from
//! the window before, and the windows are faded into each other.
//!
//! It runs in the mixer, on the output's real-time thread, so it allocates
//! only as it's made: for a rate, and the fastest speed there is.

use super::audio::SOURCE_CHANNELS;

/// The slowest a file plays.
pub const MIN_SPEED: f64 = 0.25;
/// The fastest a file plays.
pub const MAX_SPEED: f64 = 4.0;

/// How long each window is, in seconds.
const WINDOW_SECONDS: f64 = 0.04;
/// How far either way from where the speed says a window may be taken,
/// in seconds.
const SEARCH_SECONDS: f64 = 0.01;
/// Every how many frames the first, rough search looks, and compares.
const ROUGH: usize = 4;

/// Sound fed in at one speed, heard at another.
pub struct Stretch {
    rate: u32,
    /// Frames in a window; even.
    window: usize,
    /// Frames between windows heard: half a window.
    hop: usize,
    /// Frames either way a window may be taken from.
    search: usize,
    /// How each window fades in and out, a frame each; any two half a
    /// window apart add up to one.
    fade: Vec<f32>,
    /// The sound fed in and still needed, interleaved stereo; room for
    /// `capacity` frames.
    input: Vec<f32>,
    /// Frames in `input`.
    held: usize,
    /// Media time of the first frame in `input`.
    input_pts: f64,
    /// Where the next window is due from, in frames into `input`.
    next: f64,
    /// Where the sound goes on from the last window taken, in frames into
    /// `input`, which the next is matched against; none for the first.
    after: Option<usize>,
    /// The windows faded together; its first half is heard next.
    sum: Vec<f32>,
    /// Frames of `sum`'s first half ready to be heard, and how many are.
    ready: usize,
    taken: usize,
    /// Media time of the first frame ready, and how fast what's ready plays.
    ready_pts: f64,
    ready_speed: f64,
}

impl Stretch {
    pub fn new(rate: u32) -> Self {
        let window = ((f64::from(rate) * WINDOW_SECONDS) as usize / 2 * 2).max(2 * ROUGH);
        let hop = window / 2;
        let search = (f64::from(rate) * SEARCH_SECONDS) as usize;
        let fade = (0..window)
            .map(|i| {
                let phase = std::f64::consts::TAU * i as f64 / window as f64;
                (0.5 - 0.5 * phase.cos()) as f32
            })
            .collect();
        let capacity = 2 * search + (MAX_SPEED.ceil() as usize + 1) * hop + window + 2;
        Self {
            rate,
            window,
            hop,
            search,
            fade,
            input: vec![0.0; capacity * SOURCE_CHANNELS],
            held: 0,
            input_pts: 0.0,
            next: 0.0,
            after: None,
            sum: vec![0.0; window * SOURCE_CHANNELS],
            ready: 0,
            taken: 0,
            ready_pts: 0.0,
            ready_speed: 1.0,
        }
    }

    /// Whether it holds any sound: once it has, what plays goes through it
    /// until it's reset, at any speed, so none is lost.
    pub fn is_active(&self) -> bool {
        self.held > 0 || self.ready > self.taken
    }

    /// Let go of everything fed in, to start again elsewhere.
    pub fn reset(&mut self) {
        self.held = 0;
        self.next = 0.0;
        self.after = None;
        self.sum.fill(0.0);
        self.ready = 0;
        self.taken = 0;
    }

    /// How many more frames it needs before the next window at `speed`,
    /// once what's ready has been heard.
    pub fn wants(&self) -> usize {
        self.needed().saturating_sub(self.held)
    }

    fn needed(&self) -> usize {
        let nominal = self.next.round() as usize;
        let last = nominal + self.search + self.window;
        self.after.map_or(last, |after| last.max(after + self.hop))
    }

    /// Take `frames` (interleaved stereo) of what plays from `pts`, if it
    /// starts afresh, or just after what it has, up to what it wants.
    /// How many frames it took.
    pub fn feed(&mut self, pts: f64, frames: &[f32]) -> usize {
        if self.held == 0 {
            self.input_pts = pts;
        }
        let room = self.input.len() / SOURCE_CHANNELS - self.held;
        let count = (frames.len() / SOURCE_CHANNELS).min(self.wants()).min(room);
        let at = self.held * SOURCE_CHANNELS;
        self.input[at..at + count * SOURCE_CHANNELS]
            .copy_from_slice(&frames[..count * SOURCE_CHANNELS]);
        self.held += count;
        count
    }

    /// What's ready to be heard, interleaved stereo.
    pub fn ready(&self) -> &[f32] {
        &self.sum[self.taken * SOURCE_CHANNELS..self.ready * SOURCE_CHANNELS]
    }

    /// Note `frames` of what's ready as heard.
    pub fn take(&mut self, frames: usize) {
        self.taken = self.taken.saturating_add(frames).min(self.ready);
    }

    /// Media time of the next frame to be heard: the first ready, else the
    /// next window's. None while it holds nothing.
    pub fn read_pts(&self) -> Option<f64> {
        if self.taken < self.ready {
            let heard = self.taken as f64 * self.ready_speed / f64::from(self.rate);
            Some(self.ready_pts + heard)
        } else if self.held > 0 {
            Some(self.input_pts + self.next / f64::from(self.rate))
        } else {
            None
        }
    }

    /// Fade the next window in at `speed`, once what's ready has been
    /// heard, making the next half window ready. Whether it could: not
    /// before it's fed what it `wants`.
    pub fn step(&mut self, speed: f64) -> bool {
        if self.taken < self.ready || self.wants() > 0 {
            return false;
        }
        let speed = speed.clamp(MIN_SPEED, MAX_SPEED);
        if self.ready > 0 {
            // Its first half was heard: the second is the first now.
            let half = self.hop * SOURCE_CHANNELS;
            self.sum.copy_within(half.., 0);
            self.sum[half..].fill(0.0);
        }
        let nominal = self.next.round() as usize;
        let from = match self.after {
            // At its own speed, where it goes on is the window, if that's
            // near where it's due: nothing changes. Just slower or faster,
            // it's searched for near where the speed got it to.
            Some(after) if speed == 1.0 && after.abs_diff(nominal) <= self.search => {
                self.next = after as f64;
                after
            }
            Some(after) => self.best(after, nominal),
            None => nominal,
        };
        let window = &self.input[from * SOURCE_CHANNELS..(from + self.window) * SOURCE_CHANNELS];
        let (frames, _) = window.as_chunks::<SOURCE_CHANNELS>();
        let (sums, _) = self.sum.as_chunks_mut::<SOURCE_CHANNELS>();
        for ((sum, frame), fade) in sums.iter_mut().zip(frames).zip(&self.fade) {
            for (s, x) in sum.iter_mut().zip(frame) {
                *s += x * fade;
            }
        }
        self.ready_pts = self.input_pts + self.next / f64::from(self.rate);
        self.ready_speed = speed;
        self.ready = self.hop;
        self.taken = 0;
        self.after = Some(from + self.hop);
        self.next += self.hop as f64 * speed;
        self.let_go();
        true
    }

    /// Where near `nominal`, a search either way, the window that goes on
    /// best from the sound at `after` starts: roughly first, then exactly
    /// around that.
    fn best(&self, after: usize, nominal: usize) -> usize {
        let low = nominal.saturating_sub(self.search);
        let high = nominal + self.search;
        let rough = (low..=high)
            .step_by(ROUGH)
            .max_by(|&a, &b| {
                let a = self.likeness(after, a, ROUGH);
                let b = self.likeness(after, b, ROUGH);
                a.total_cmp(&b)
            })
            .unwrap_or(nominal);
        let near = rough.saturating_sub(ROUGH - 1).max(low)..=(rough + ROUGH - 1).min(high);
        near.max_by(|&a, &b| {
            self.likeness(after, a, 1)
                .total_cmp(&self.likeness(after, b, 1))
        })
        .unwrap_or(rough)
    }

    /// How alike the half windows at `a` and `b` sound, both channels as
    /// one, every `every` frames.
    fn likeness(&self, a: usize, b: usize, every: usize) -> f32 {
        let (frames, _) = self.input[..self.held * SOURCE_CHANNELS].as_chunks::<SOURCE_CHANNELS>();
        frames[a..a + self.hop]
            .iter()
            .step_by(every)
            .zip(frames[b..b + self.hop].iter().step_by(every))
            .map(|(x, y)| (x[0] + x[1]) * (y[0] + y[1]))
            .sum()
    }

    /// Let go of the sound before what the next window may need.
    fn let_go(&mut self) {
        let searched = (self.next.floor() as usize).saturating_sub(self.search);
        let gone = self.after.map_or(searched, |after| after.min(searched));
        if gone == 0 {
            return;
        }
        let gone = gone.min(self.held);
        self.input
            .copy_within(gone * SOURCE_CHANNELS..self.held * SOURCE_CHANNELS, 0);
        self.held -= gone;
        self.next -= gone as f64;
        self.after = self.after.map(|after| after - gone);
        self.input_pts += gone as f64 / f64::from(self.rate);
    }
}

#[cfg(test)]
#[path = "tests/tempo.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/tempo_properties.rs"]
mod properties;
