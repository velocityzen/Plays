//! Decoding audio: packets resampled to the mixer's rate, as interleaved
//! stereo `f32`, into the player's audio source.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use ffmpeg::software::resampling;
use ffmpeg::util::{channel_layout::ChannelLayout, format::Sample};
use ffmpeg::{Packet, codec, frame};
use ffmpeg_next as ffmpeg;
use ffmpeg_safe::Layout;

use super::{Session, StreamInfo, codec_name};
use crate::media::audio::{AudioChunk, AudioSource, SOURCE_CHANNELS};

/// Decoded audio kept ahead of the playhead.
const AUDIO_QUEUE_SECONDS: f64 = 0.75;
/// The least the decode thread waits for the mixer to make room for more
/// sound: about a callback's worth, should the mixer stop taking it.
const AUDIO_ROOM_WAIT: Duration = Duration::from_millis(10);
/// A gap in the sound's timestamps longer than this, in seconds, plays as
/// silence, so the clock, which follows the sound, doesn't jump the
/// picture over it.
const GAP: f64 = 0.1;
/// The most silence a gap plays as, in seconds: the clock jumps the rest.
const MOST_SILENCE: f64 = 10.0;
/// The fewest samples a second sound can have to play. Telephones take
/// 8000; fewer than this is a broken or hostile file, a frame of which,
/// resampled, would take gigabytes.
const LEAST_RATE: u32 = 1000;

pub(super) struct AudioDecoder {
    pub(super) index: usize,
    pub(super) decoder: ffmpeg::decoder::Audio,
    source: Arc<AudioSource>,
    pub(super) time_base: f64,
    /// The resampler, and the format, layout and rate it takes.
    resampler: Option<(resampling::Context, (Sample, Layout, u32))>,
    decoded: frame::Audio,
    next_pts: Option<f64>,
    /// Its sound is due from where playing started, or a seek went, so a
    /// gap before its first plays as silence: not when where the media
    /// starts isn't known, nor for a track picked as a live stream plays.
    pub(super) due_from_the_start: bool,
    pub(super) finished: bool,
}

const OUTPUT_FORMAT: Sample = Sample::F32(ffmpeg::util::format::sample::Type::Packed);

impl AudioDecoder {
    pub(super) fn new(stream: &StreamInfo, source: Arc<AudioSource>) -> Result<Self> {
        let context = codec::context::Context::from_parameters(stream.parameters.clone())?;
        let decoder = context
            .decoder()
            .audio()
            .context("opening the audio decoder")?;
        let rate = decoder.rate();
        // Not yet known is learned from the first frame.
        if rate != 0 && rate < LEAST_RATE {
            bail!("its sound has {rate} samples a second, too few to play");
        }
        Ok(Self {
            index: stream.index,
            decoder,
            source,
            time_base: f64::from(stream.time_base),
            resampler: None,
            decoded: frame::Audio::empty(),
            next_pts: None,
            due_from_the_start: false,
            finished: false,
        })
    }

    pub(super) fn codec_name(&self) -> String {
        codec_name(self.decoder.id())
    }

    fn output_rate(&self) -> u32 {
        self.source.sample_rate()
    }

    /// How fast it plays, which it decodes ahead for.
    fn speed(&self) -> f64 {
        f64::from(self.source.speed.load())
    }

    /// Whether there's room for more: as much as plays for as long at its
    /// speed, faster or not.
    pub(super) fn has_room(&self) -> bool {
        let ahead = AUDIO_QUEUE_SECONDS * self.speed().max(1.0);
        self.source.queue.lock().buffered_seconds() < ahead
    }

    /// How soon the mixer, playing, makes room for more.
    pub(super) fn until_room(&self) -> Option<Duration> {
        if !self.source.playing.load(Ordering::Relaxed) {
            return None;
        }
        let speed = self.speed();
        let ahead = AUDIO_QUEUE_SECONDS * speed.max(1.0);
        let over = (self.source.queue.lock().buffered_seconds() - ahead) / speed;
        Some(Duration::from_secs_f64(over.max(0.0)).max(AUDIO_ROOM_WAIT))
    }

    pub(super) fn reset(&mut self) {
        self.decoder.flush();
        self.resampler = None;
        self.next_pts = None;
        self.due_from_the_start = true;
        self.finished = false;
    }

    pub(super) fn decode(&mut self, packet: &Packet, session: &Session) {
        if let Err(err) = self.decoder.send_packet(packet) {
            log::debug!("audio decode error: {err}");
        }
        self.drain(session);
    }

    pub(super) fn finish(&mut self, session: &Session) {
        let _ = self.decoder.send_eof();
        self.drain(session);
        self.finished = true;
        session.shared.set_audio_ended(session.serial);
    }

    fn drain(&mut self, session: &Session) {
        while self.decoder.receive_frame(&mut self.decoded).is_ok() {
            if let Err(err) = self.process(session) {
                log::warn!("audio conversion failed: {err:#}");
            }
        }
    }

    fn process(&mut self, session: &Session) -> Result<()> {
        let out_rate = self.output_rate();
        let in_rate = self.decoded.rate();
        if in_rate < LEAST_RATE {
            bail!("sound with {in_rate} samples a second, too few to play");
        }
        let in_samples = self.decoded.samples();
        if in_samples == 0 {
            return Ok(());
        }

        let input = (
            self.decoded.format(),
            Layout::of(&mut self.decoded),
            in_rate,
        );
        // Rebuilt for a new input format, or a new output rate (the mixer's
        // for another device).
        if self
            .resampler
            .as_ref()
            .is_none_or(|(r, taken)| *taken != input || r.output().rate != out_rate)
        {
            let resampler = resampling::Context::get(
                input.0,
                input.1.lent(),
                input.2,
                OUTPUT_FORMAT,
                ChannelLayout::STEREO,
                out_rate,
            )?;
            self.resampler = Some((resampler, input));
        }

        let pts = self
            .decoded
            .timestamp()
            .map(|ts| ts as f64 * self.time_base - session.start_offset())
            .or(self.next_pts)
            .unwrap_or(0.0);
        let due = self.next_pts.or_else(|| {
            self.due_from_the_start
                .then(|| session.seek_target.unwrap_or(0.0))
        });
        self.next_pts = Some(pts + in_samples as f64 / in_rate as f64);
        if let Some(due) = due
            && pts - due > GAP
        {
            let silence = ((pts - due).min(MOST_SILENCE) * f64::from(out_rate)) as usize;
            self.push(due, vec![0.0; silence * SOURCE_CHANNELS], out_rate, session);
        }

        let (resampler, _) = self.resampler.as_mut().expect("resampler was just created");
        // Leave room for everything the resampler may still be holding.
        let pending = resampler.delay().map_or(0, |d| d.output.max(0) as usize);
        let capacity = in_samples * out_rate as usize / in_rate as usize + pending + 64;
        let mut output = frame::Audio::new(OUTPUT_FORMAT, capacity, ChannelLayout::STEREO);
        resampler.run(&self.decoded, &mut output)?;

        let frames = output.samples();
        let bytes = &output.data(0)[..frames * SOURCE_CHANNELS * 4];
        let samples: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_ne_bytes(*b))
            .collect();

        self.push(pts, samples, out_rate, session);
        Ok(())
    }

    /// Queue `samples`, at `rate` from `pts`, but for what's before where
    /// the last seek went.
    fn push(&self, mut pts: f64, mut samples: Vec<f32>, rate: u32, session: &Session) {
        if let Some(target) = session.seek_target
            && pts < target
        {
            let frames = samples.len() / SOURCE_CHANNELS;
            let skip = (((target - pts) * f64::from(rate)) as usize).min(frames);
            samples.drain(..skip * SOURCE_CHANNELS);
            pts += skip as f64 / f64::from(rate);
        }
        if !samples.is_empty() {
            self.source.push(AudioChunk {
                pts,
                samples,
                pos: 0,
                rate,
            });
        }
    }
}
