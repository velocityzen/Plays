//! The mixers players play into: one for each output.
//!
//! Decoders resample their audio to interleaved stereo `f32` at their
//! mixer's sample rate and push it into an [`AudioSource`]. The output
//! (`output.rs`) has the mixer sum all playing sources, apply per-player and
//! master volume, and report what it is playing back to each player's
//! [`Clock`] so video can follow the audio. A new output device may want
//! another sample rate, and a player may move to another mixer: either way
//! what is queued is converted to the new rate, and decoders resample to it
//! from then on.
//!
//! The mixer runs on the output's real-time thread, so it neither allocates
//! nor frees, and waits for no other thread: what other threads do holding
//! the locks it takes is short, and a lock held longer than that it goes
//! on without, that player silent for the moment.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Mutex, MutexGuard};

use super::clock::Clock;

/// Channels every source delivers (interleaved L/R).
pub const SOURCE_CHANNELS: usize = 2;
/// Chunks a queue has room for from the start, so it seldom grows.
const QUEUE_CHUNKS: usize = 128;
/// Played chunks a decoder lets go of each time it queues another.
const LET_GO_AT_ONCE: usize = 8;
/// Players a mixer has room for from the start.
const SOURCES: usize = 16;
/// How long the mixer tries for a lock another thread has before going on
/// without it: what others do holding one takes far less, unless the
/// system put them to sleep meanwhile.
const LOCK_REACH: Duration = Duration::from_micros(50);

/// An `f32` that can be shared between threads without a lock.
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn store(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed)
    }
}

pub struct AudioChunk {
    /// Media time of the first sample in `samples`.
    pub pts: f64,
    /// Interleaved stereo samples.
    pub samples: Vec<f32>,
    /// Read position, in frames (one frame = one sample per channel).
    pub pos: usize,
    /// The sample rate the samples were resampled to.
    pub rate: u32,
}

impl AudioChunk {
    fn frames(&self) -> usize {
        self.samples.len() / SOURCE_CHANNELS
    }

    fn played(&self) -> bool {
        self.pos >= self.frames()
    }

    /// What is left to play of it, at `rate`.
    fn resampled(&self, rate: u32) -> Self {
        let rest = &self.samples[self.pos * SOURCE_CHANNELS..];
        Self {
            pts: self.pts + self.pos as f64 / f64::from(self.rate),
            samples: resample(rest, self.rate, rate),
            pos: 0,
            rate,
        }
    }
}

/// Interleaved stereo `samples` at `from` Hz, at `to` Hz instead. Linear:
/// only what was already queued when the output changed goes through it;
/// decoders resample everything else properly.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    let frames = samples.len() / SOURCE_CHANNELS;
    if from == to || frames == 0 {
        return samples.to_vec();
    }
    let out_frames = (frames as u64 * u64::from(to) / u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    let (input, _) = samples.as_chunks::<SOURCE_CHANNELS>();
    let mut out = Vec::with_capacity(out_frames * SOURCE_CHANNELS);
    for i in 0..out_frames {
        let x = i as f64 * step;
        let at = (x as usize).min(frames - 1);
        let next = (at + 1).min(frames - 1);
        let t = (x - at as f64) as f32;
        for (a, b) in input[at].iter().zip(&input[next]) {
            out.push(a + (b - a) * t);
        }
    }
    out
}

pub struct AudioQueue {
    /// What was decoded, in order. Chunks played through stay at the front
    /// until the decoder lets go of them, so the mixer frees nothing.
    pub chunks: VecDeque<AudioChunk>,
    /// Seek generation the queued samples belong to.
    pub serial: u64,
    /// Frames still waiting to be played.
    pub buffered_frames: usize,
    /// The sample rate the mixer plays at: every chunk is at it.
    pub rate: u32,
}

impl AudioQueue {
    pub fn new(rate: u32) -> Self {
        Self {
            chunks: VecDeque::with_capacity(QUEUE_CHUNKS),
            serial: 0,
            buffered_frames: 0,
            rate,
        }
    }

    /// Start again for `serial`, queueing into `fresh`; what was queued,
    /// to let go of once the lock is.
    fn clear(&mut self, serial: u64, fresh: VecDeque<AudioChunk>) -> VecDeque<AudioChunk> {
        self.buffered_frames = 0;
        self.serial = serial;
        std::mem::replace(&mut self.chunks, fresh)
    }

    /// Up to `LET_GO_AT_ONCE` chunks played through, off the front.
    fn take_played(&mut self) -> [Option<AudioChunk>; LET_GO_AT_ONCE] {
        let mut played = [const { None }; LET_GO_AT_ONCE];
        for slot in &mut played {
            if !self.chunks.front().is_some_and(AudioChunk::played) {
                break;
            }
            *slot = self.chunks.pop_front();
        }
        played
    }

    /// Queue `chunk`, converted to the queue's rate if the mixer changed
    /// rate while it was being decoded.
    pub fn push(&mut self, chunk: AudioChunk) {
        let chunk = if chunk.rate == self.rate {
            chunk
        } else {
            chunk.resampled(self.rate)
        };
        self.buffered_frames += chunk.frames() - chunk.pos;
        self.chunks.push_back(chunk);
    }

    /// Pass over what plays before `time`, unheard.
    fn skip_to(&mut self, time: f64) {
        let rate = f64::from(self.rate);
        for chunk in self.chunks.iter_mut().filter(|chunk| !chunk.played()) {
            let left = chunk.frames() - chunk.pos;
            let from = chunk.pts + chunk.pos as f64 / rate;
            let skip = (((time - from) * rate).max(0.0) as usize).min(left);
            chunk.pos += skip;
            self.buffered_frames -= skip;
            if skip < left {
                break;
            }
        }
    }

    /// Convert what is queued to `rate`, which the mixer plays at now.
    fn set_rate(&mut self, rate: u32) {
        if rate == self.rate {
            return;
        }
        for chunk in &mut self.chunks {
            *chunk = chunk.resampled(rate);
        }
        self.buffered_frames = self.chunks.iter().map(AudioChunk::frames).sum();
        self.rate = rate;
    }

    /// Seconds still waiting to be played.
    pub fn buffered_seconds(&self) -> f64 {
        self.buffered_frames as f64 / f64::from(self.rate)
    }

    /// Media time of the next frame that will be read.
    fn read_pts(&self) -> Option<f64> {
        self.chunks
            .iter()
            .find(|chunk| !chunk.played())
            .map(|chunk| chunk.pts + chunk.pos as f64 / f64::from(self.rate))
    }
}

/// One player's connection to the mixer.
pub struct AudioSource {
    /// What the decoder has resampled, at the mixer's rate (which its
    /// decoder must resample to).
    pub queue: Mutex<AudioQueue>,
    /// The seek generation the player currently wants to hear.
    pub serial: Arc<AtomicU64>,
    pub clock: Arc<Clock>,
    pub playing: AtomicBool,
    pub volume: AtomicF32,
    pub muted: AtomicBool,
    /// How much later than the picture the sound is heard, in seconds
    /// (negative: earlier). The clock, which the picture follows, is kept
    /// that far ahead of what is heard.
    pub delay: AtomicF32,
    /// Peak level of the last callback, for the level meter.
    pub peak: AtomicF32,
    /// Its sound plays on a device a cast sends it to, which keeps the
    /// clock: what is queued here goes unheard as the clock passes it.
    pub elsewhere: AtomicBool,
}

impl AudioSource {
    pub fn new(clock: Arc<Clock>, serial: Arc<AtomicU64>, sample_rate: u32, volume: f32) -> Self {
        Self {
            queue: Mutex::new(AudioQueue::new(sample_rate)),
            serial,
            clock,
            playing: AtomicBool::new(false),
            volume: AtomicF32::new(volume),
            muted: AtomicBool::new(false),
            delay: AtomicF32::new(0.0),
            peak: AtomicF32::new(0.0),
            elsewhere: AtomicBool::new(false),
        }
    }

    /// The rate to resample to: the mixer's.
    pub fn sample_rate(&self) -> u32 {
        self.queue.lock().rate
    }

    /// Queue `chunk`, letting go of some that played. Should the mixer
    /// have changed rate since it was decoded, it is converted with the
    /// lock let go.
    pub fn push(&self, mut chunk: AudioChunk) {
        let played = loop {
            let mut queue = self.queue.lock();
            if chunk.rate == queue.rate {
                let played = queue.take_played();
                queue.push(chunk);
                break played;
            }
            let rate = queue.rate;
            drop(queue);
            chunk = chunk.resampled(rate);
        };
        drop(played);
    }

    /// Start again for the seek `serial`: what was queued is let go of.
    pub fn clear(&self, serial: u64) {
        let fresh = VecDeque::with_capacity(QUEUE_CHUNKS);
        let old = self.queue.lock().clear(serial, fresh);
        drop(old);
    }
}

/// `lock`, unless another thread keeps it longer than `LOCK_REACH`.
fn within_reach<T>(lock: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    let start = Instant::now();
    loop {
        if let Some(guard) = lock.try_lock() {
            return Some(guard);
        }
        if start.elapsed() >= LOCK_REACH {
            return None;
        }
        std::hint::spin_loop();
    }
}

/// State shared between the UI thread, decoder threads and the audio callback.
pub struct Mixer {
    sample_rate: AtomicU32,
    sources: Mutex<Vec<Arc<AudioSource>>>,
    master_volume: AtomicF32,
}

impl Mixer {
    pub fn new(sample_rate: u32, master_volume: f32) -> Arc<Self> {
        Arc::new(Self {
            sample_rate: AtomicU32::new(sample_rate),
            sources: Mutex::new(Vec::with_capacity(SOURCES)),
            master_volume: AtomicF32::new(master_volume.clamp(0.0, 1.0)),
        })
    }

    /// The rate it plays at, and players resample to.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Acquire)
    }

    /// Play at `rate` from now on, for an output that wants it: what every
    /// player has queued is converted to it.
    pub fn set_sample_rate(&self, rate: u32) {
        self.sample_rate.store(rate, Ordering::Release);
        // Converted with the list let go, which the mixer needs.
        let sources = self.sources.lock().clone();
        for source in sources {
            source.queue.lock().set_rate(rate);
        }
    }

    /// Mix `source` in, converting what it has queued to this mixer's
    /// rate, as it may come from another.
    pub fn add_source(&self, source: Arc<AudioSource>) {
        loop {
            // Converted before it is mixed in, holding nothing up.
            let rate = self.sample_rate();
            source.queue.lock().set_rate(rate);
            let mut sources = self.sources.lock();
            // A change of rate meanwhile would have passed it by.
            if self.sample_rate() == rate {
                sources.push(source);
                return;
            }
        }
    }

    pub fn remove_source(&self, source: &Arc<AudioSource>) {
        let removed = {
            let mut sources = self.sources.lock();
            let at = sources.iter().position(|s| Arc::ptr_eq(s, source));
            at.map(|at| sources.remove(at))
        };
        drop(removed);
    }

    pub fn set_master_volume(&self, volume: f32) {
        self.master_volume.store(volume.clamp(0.0, 1.0));
    }

    /// A mixer with no audio device behind it; tests pull samples with [`Mixer::mix`].
    #[cfg(test)]
    pub fn detached(sample_rate: u32) -> Arc<Self> {
        Self::new(sample_rate, 1.0)
    }

    /// Fill `mix` (interleaved stereo) with the sum of all playing sources.
    pub(crate) fn mix(&self, mix: &mut [f32], latency: f64, now: Instant) {
        mix.fill(0.0);
        let frames = mix.len() / SOURCE_CHANNELS;
        let master = self.master_volume.load();

        let Some(sources) = within_reach(&self.sources) else {
            return;
        };
        for source in sources.iter() {
            if !source.playing.load(Ordering::Relaxed) {
                source.peak.store(0.0);
                continue;
            }
            let Some(mut queue) = within_reach(&source.queue) else {
                source.peak.store(0.0);
                continue;
            };
            let queue = &mut *queue;
            // Samples from before the latest seek must never be heard.
            if queue.serial != source.serial.load(Ordering::Acquire) {
                continue;
            }
            let delay = f64::from(source.delay.load());
            if source.elsewhere.load(Ordering::Relaxed) {
                if let Some(now) = source.clock.try_now() {
                    queue.skip_to(now - delay);
                }
                source.peak.store(0.0);
                continue;
            }
            if let Some(pts) = queue.read_pts() {
                // What we write now is heard `latency` seconds from now,
                // and goes with the picture `delay` seconds later.
                source.clock.sync(pts - latency + delay, now);
            }

            // The meter shows the player's own level, before the master volume.
            let level_gain = if source.muted.load(Ordering::Relaxed) {
                0.0
            } else {
                source.volume.load()
            };
            let gain = level_gain * master;
            let mut peak = 0.0f32;
            let mut written = 0;
            for chunk in queue.chunks.iter_mut().filter(|chunk| !chunk.played()) {
                if written == frames {
                    break;
                }
                let n = (chunk.frames() - chunk.pos).min(frames - written);
                let src =
                    &chunk.samples[chunk.pos * SOURCE_CHANNELS..(chunk.pos + n) * SOURCE_CHANNELS];
                let dst = &mut mix[written * SOURCE_CHANNELS..(written + n) * SOURCE_CHANNELS];
                for (d, s) in dst.iter_mut().zip(src) {
                    peak = peak.max(s.abs());
                    *d += s * gain;
                }
                chunk.pos += n;
                written += n;
                queue.buffered_frames -= n;
            }
            source.peak.store(peak * level_gain);
        }
    }
}

#[cfg(test)]
#[path = "tests/audio.rs"]
mod tests;
