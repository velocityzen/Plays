//! The mixer every open player plays into.
//!
//! Decoders resample their audio to interleaved stereo `f32` at the mixer's
//! sample rate and push it into an [`AudioSource`]. The output (`output.rs`)
//! has the mixer sum all playing sources, apply per-player and master volume,
//! and report what it is playing back to each player's [`Clock`] so video can
//! follow the audio. A new output device may want another sample rate: then
//! what is queued is converted to it, and decoders resample to it from then on.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use parking_lot::Mutex;

use super::clock::Clock;

/// Channels every source delivers (interleaved L/R).
pub const SOURCE_CHANNELS: usize = 2;

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
            chunks: VecDeque::new(),
            serial: 0,
            buffered_frames: 0,
            rate,
        }
    }

    pub fn clear(&mut self, serial: u64) {
        self.chunks.clear();
        self.buffered_frames = 0;
        self.serial = serial;
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

    /// Let go of what plays before `time`, unheard.
    fn skip_to(&mut self, time: f64) {
        let rate = f64::from(self.rate);
        while let Some(chunk) = self.chunks.front_mut() {
            let left = chunk.frames() - chunk.pos;
            let from = chunk.pts + chunk.pos as f64 / rate;
            let skip = (((time - from) * rate).max(0.0) as usize).min(left);
            chunk.pos += skip;
            self.buffered_frames -= skip;
            if skip < left {
                break;
            }
            self.chunks.pop_front();
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
            .front()
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
            sources: Mutex::new(Vec::new()),
            master_volume: AtomicF32::new(master_volume),
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
        for source in self.sources.lock().iter() {
            source.queue.lock().set_rate(rate);
        }
    }

    pub fn add_source(&self, source: Arc<AudioSource>) {
        self.sources.lock().push(source);
    }

    pub fn remove_source(&self, source: &Arc<AudioSource>) {
        self.sources.lock().retain(|s| !Arc::ptr_eq(s, source));
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

        for source in self.sources.lock().iter() {
            if !source.playing.load(Ordering::Relaxed) {
                source.peak.store(0.0);
                continue;
            }
            let mut queue = source.queue.lock();
            // Samples from before the latest seek must never be heard.
            if queue.serial != source.serial.load(Ordering::Acquire) {
                continue;
            }
            let delay = f64::from(source.delay.load());
            if source.elsewhere.load(Ordering::Relaxed) {
                queue.skip_to(source.clock.now() - delay);
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
            while written < frames {
                let Some(chunk) = queue.chunks.front_mut() else {
                    break;
                };
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
                let done = chunk.pos >= chunk.frames();
                queue.buffered_frames -= n;
                if done {
                    queue.chunks.pop_front();
                }
            }
            source.peak.store(peak * level_gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `frames` of stereo, each frame's samples its index.
    fn ramp(frames: usize, pts: f64, rate: u32) -> AudioChunk {
        AudioChunk {
            pts,
            samples: (0..frames).flat_map(|i| [i as f32; 2]).collect(),
            pos: 0,
            rate,
        }
    }

    #[test]
    fn resamples_linearly() {
        let ramp: Vec<f32> = (0..8).flat_map(|i| [i as f32, -(i as f32)]).collect();
        let up = resample(&ramp, 24_000, 48_000);
        assert_eq!(up.len(), 32);
        assert_eq!(&up[..6], &[0.0, 0.0, 0.5, -0.5, 1.0, -1.0]);
        let down = resample(&ramp, 48_000, 24_000);
        assert_eq!(down, [0.0, 0.0, 2.0, -2.0, 4.0, -4.0, 6.0, -6.0]);
        assert_eq!(resample(&ramp, 48_000, 48_000), ramp);
    }

    #[test]
    fn what_is_queued_follows_the_mixer_to_another_rate() {
        let mut queue = AudioQueue::new(48_000);
        let mut chunk = ramp(4800, 1.0, 48_000);
        chunk.pos = 2400;
        queue.push(chunk);
        assert_eq!(queue.buffered_frames, 2400);
        assert_eq!(queue.read_pts(), Some(1.05));

        queue.set_rate(24_000);
        assert_eq!(queue.rate, 24_000);
        assert_eq!(queue.buffered_frames, 1200);
        assert_eq!(queue.read_pts(), Some(1.05));
        assert!((queue.buffered_seconds() - 0.05).abs() < 1e-9);
        // It goes on from where it was: frame 2400, then every other one.
        let chunk = queue.chunks.front().unwrap();
        assert_eq!(&chunk.samples[..4], &[2400.0, 2400.0, 2402.0, 2402.0]);

        // Decoded at the old rate while it changed: converted on the way in.
        queue.push(ramp(480, 1.1, 48_000));
        assert_eq!(queue.buffered_frames, 1200 + 240);
        assert_eq!(queue.chunks.back().unwrap().rate, 24_000);
    }

    #[test]
    fn sound_heard_elsewhere_is_let_go_as_the_clock_passes_it_unheard() {
        let mixer = Mixer::detached(48_000);
        let clock = Arc::new(Clock::new());
        let source = Arc::new(AudioSource::new(
            clock.clone(),
            Arc::new(AtomicU64::new(0)),
            mixer.sample_rate(),
            1.0,
        ));
        mixer.add_source(source.clone());
        source.playing.store(true, Ordering::Relaxed);
        source.elsewhere.store(true, Ordering::Relaxed);
        source.queue.lock().push(ramp(48_000, 0.0, 48_000));
        clock.set(0.25);
        let mut mix = vec![1.0; 960];
        mixer.mix(&mut mix, 0.0, Instant::now());

        assert!(mix.iter().all(|&sample| sample == 0.0), "unheard");
        assert_eq!(source.peak.load(), 0.0);
        assert_eq!(clock.now(), 0.25, "the clock goes by the sound elsewhere");
        let queue = source.queue.lock();
        assert_eq!(queue.buffered_frames, 36_000, "what played went");
        assert_eq!(queue.read_pts(), Some(0.25));
    }

    #[test]
    fn a_mixer_changes_every_player_to_its_rate() {
        let mixer = Mixer::detached(48_000);
        let source = Arc::new(AudioSource::new(
            Arc::new(Clock::new()),
            Arc::new(AtomicU64::new(0)),
            mixer.sample_rate(),
            1.0,
        ));
        mixer.add_source(source.clone());
        source.queue.lock().push(ramp(4800, 0.0, 48_000));
        mixer.set_sample_rate(44_100);
        assert_eq!(mixer.sample_rate(), 44_100);
        assert_eq!(source.sample_rate(), 44_100);
        assert_eq!(source.queue.lock().buffered_frames, 4410);
    }
}
