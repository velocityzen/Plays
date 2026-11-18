//! A single audio output stream that mixes every open player.
//!
//! Decoders resample their audio to interleaved stereo `f32` at the mixer's
//! sample rate and push it into an [`AudioSource`]. The output callback sums all
//! playing sources, applies per-player and master volume, and reports what it is
//! playing back to each player's [`Clock`] so video can follow the audio.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
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
}

impl AudioChunk {
    fn frames(&self) -> usize {
        self.samples.len() / SOURCE_CHANNELS
    }
}

#[derive(Default)]
pub struct AudioQueue {
    pub chunks: VecDeque<AudioChunk>,
    /// Seek generation the queued samples belong to.
    pub serial: u64,
    /// Frames still waiting to be played.
    pub buffered_frames: usize,
}

impl AudioQueue {
    pub fn clear(&mut self, serial: u64) {
        self.chunks.clear();
        self.buffered_frames = 0;
        self.serial = serial;
    }

    pub fn push(&mut self, chunk: AudioChunk) {
        self.buffered_frames += chunk.frames() - chunk.pos;
        self.chunks.push_back(chunk);
    }

    /// Media time of the next frame that will be read.
    fn read_pts(&self, sample_rate: u32) -> Option<f64> {
        self.chunks
            .front()
            .map(|chunk| chunk.pts + chunk.pos as f64 / sample_rate as f64)
    }
}

/// One player's connection to the mixer.
pub struct AudioSource {
    /// Rate the decoder must resample to (the mixer's output rate).
    pub sample_rate: u32,
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
}

impl AudioSource {
    pub fn new(clock: Arc<Clock>, serial: Arc<AtomicU64>, sample_rate: u32, volume: f32) -> Self {
        Self {
            sample_rate,
            queue: Mutex::new(AudioQueue::default()),
            serial,
            clock,
            playing: AtomicBool::new(false),
            volume: AtomicF32::new(volume),
            muted: AtomicBool::new(false),
            delay: AtomicF32::new(0.0),
            peak: AtomicF32::new(0.0),
        }
    }
}

/// State shared between the UI thread, decoder threads and the audio callback.
pub struct Mixer {
    pub sample_rate: u32,
    pub output_channels: usize,
    pub device_name: String,
    sources: Mutex<Vec<Arc<AudioSource>>>,
    master_volume: AtomicF32,
}

impl Mixer {
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
        Arc::new(Self {
            sample_rate,
            output_channels: SOURCE_CHANNELS,
            device_name: "test".into(),
            sources: Mutex::new(Vec::new()),
            master_volume: AtomicF32::new(1.0),
        })
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
            if let Some(pts) = queue.read_pts(self.sample_rate) {
                // What we write now is heard `latency` seconds from now,
                // and goes with the picture `delay` seconds later.
                let delay = f64::from(source.delay.load());
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

/// Owns the platform audio stream. Not `Send`, so it lives on the UI thread.
pub struct AudioOutput {
    pub mixer: Arc<Mixer>,
    _stream: cpal::Stream,
}

impl AudioOutput {
    pub fn new(master_volume: f32) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no audio output device"))?;
        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "Default output".into());
        let supported = device
            .default_output_config()
            .context("querying the default output config")?;
        let sample_format = supported.sample_format();
        let config = supported.config();

        let mixer = Arc::new(Mixer {
            sample_rate: config.sample_rate,
            output_channels: config.channels as usize,
            device_name,
            sources: Mutex::new(Vec::new()),
            master_volume: AtomicF32::new(master_volume),
        });

        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, config, mixer.clone()),
            SampleFormat::F64 => build_stream::<f64>(&device, config, mixer.clone()),
            SampleFormat::I16 => build_stream::<i16>(&device, config, mixer.clone()),
            SampleFormat::I32 => build_stream::<i32>(&device, config, mixer.clone()),
            SampleFormat::U16 => build_stream::<u16>(&device, config, mixer.clone()),
            SampleFormat::U8 => build_stream::<u8>(&device, config, mixer.clone()),
            SampleFormat::I8 => build_stream::<i8>(&device, config, mixer.clone()),
            other => Err(anyhow!("unsupported output sample format {other:?}")),
        }?;
        stream.play().context("starting the audio stream")?;

        log::info!(
            "audio output: {} @ {} Hz, {} channels ({sample_format:?})",
            mixer.device_name,
            mixer.sample_rate,
            mixer.output_channels
        );
        Ok(Self {
            mixer,
            _stream: stream,
        })
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mixer: Arc<Mixer>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate as f64;
    let mut mix = Vec::<f32>::new();

    let stream = device.build_output_stream::<T, _, _>(
        config,
        move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
            let frames = out.len() / channels;
            mix.resize(frames * SOURCE_CHANNELS, 0.0);

            let timestamp = info.timestamp();
            let latency = timestamp
                .playback
                .checked_duration_since(timestamp.callback)
                .map(|d| d.as_secs_f64())
                .unwrap_or(frames as f64 / sample_rate);
            mixer.mix(&mut mix, latency, Instant::now());

            for (frame, lr) in out
                .chunks_exact_mut(channels)
                .zip(mix.as_chunks::<SOURCE_CHANNELS>().0)
            {
                let l = lr[0].clamp(-1.0, 1.0);
                let r = lr[1].clamp(-1.0, 1.0);
                if channels == 1 {
                    frame[0] = T::from_sample((l + r) * 0.5);
                } else {
                    frame[0] = T::from_sample(l);
                    frame[1] = T::from_sample(r);
                    for extra in &mut frame[2..] {
                        *extra = T::from_sample(0.0f32);
                    }
                }
            }
        },
        |err| log::error!("audio stream error: {err}"),
        None,
    )?;
    Ok(stream)
}
