//! Decoding audio: packets resampled to the mixer's rate, as interleaved
//! stereo `f32`, into the player's audio source.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Context as _, Result};
use ffmpeg::software::resampling;
use ffmpeg::util::{channel_layout::ChannelLayout, format::Sample};
use ffmpeg::{Packet, codec, frame};
use ffmpeg_next as ffmpeg;

use super::{Session, StreamInfo, codec_name};
use crate::media::audio::{AudioChunk, AudioSource, SOURCE_CHANNELS};

/// Decoded audio kept ahead of the playhead.
const AUDIO_QUEUE_SECONDS: f64 = 0.75;
/// The least the decode thread waits for the mixer to make room for more
/// sound: about a callback's worth, should the mixer stop taking it.
const AUDIO_ROOM_WAIT: Duration = Duration::from_millis(10);

pub(super) struct AudioDecoder {
    pub(super) index: usize,
    pub(super) decoder: ffmpeg::decoder::Audio,
    source: Arc<AudioSource>,
    pub(super) time_base: f64,
    resampler: Option<resampling::Context>,
    decoded: frame::Audio,
    next_pts: Option<f64>,
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
        Ok(Self {
            index: stream.index,
            decoder,
            source,
            time_base: f64::from(stream.time_base),
            resampler: None,
            decoded: frame::Audio::empty(),
            next_pts: None,
            finished: false,
        })
    }

    pub(super) fn codec_name(&self) -> String {
        codec_name(self.decoder.id())
    }

    fn output_rate(&self) -> u32 {
        self.source.sample_rate()
    }

    pub(super) fn has_room(&self) -> bool {
        self.source.queue.lock().buffered_seconds() < AUDIO_QUEUE_SECONDS
    }

    /// How soon the mixer, playing, makes room for more.
    pub(super) fn until_room(&self) -> Option<Duration> {
        if !self.source.playing.load(Ordering::Relaxed) {
            return None;
        }
        let over = self.source.queue.lock().buffered_seconds() - AUDIO_QUEUE_SECONDS;
        Some(Duration::from_secs_f64(over.max(0.0)).max(AUDIO_ROOM_WAIT))
    }

    pub(super) fn reset(&mut self) {
        self.decoder.flush();
        self.resampler = None;
        self.next_pts = None;
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
        let in_rate = self.decoded.rate().max(1);
        let in_samples = self.decoded.samples();
        if in_samples == 0 {
            return Ok(());
        }

        // Some decoders leave the layout unspecified; resampling needs a real one.
        let mut layout = self.decoded.channel_layout();
        if layout.is_empty() || layout.channels() != i32::from(self.decoded.channels()) {
            layout = ChannelLayout::default(i32::from(self.decoded.channels()));
            self.decoded.set_channel_layout(layout);
        }
        let input = resampling::context::Definition {
            format: self.decoded.format(),
            channel_layout: layout,
            rate: in_rate,
        };
        // Rebuilt for a new input format, or a new output rate (the mixer's
        // for another device).
        if self
            .resampler
            .as_ref()
            .is_none_or(|r| *r.input() != input || r.output().rate != out_rate)
        {
            self.resampler = Some(resampling::Context::get(
                input.format,
                input.channel_layout,
                input.rate,
                OUTPUT_FORMAT,
                ChannelLayout::STEREO,
                out_rate,
            )?);
        }
        let resampler = self.resampler.as_mut().expect("resampler was just created");

        let pts = self
            .decoded
            .timestamp()
            .map(|ts| ts as f64 * self.time_base - session.start_offset())
            .or(self.next_pts)
            .unwrap_or(0.0);
        self.next_pts = Some(pts + in_samples as f64 / in_rate as f64);

        // Leave room for everything the resampler may still be holding.
        let pending = resampler.delay().map_or(0, |d| d.output.max(0) as usize);
        let capacity = in_samples * out_rate as usize / in_rate as usize + pending + 64;
        let mut output = frame::Audio::new(OUTPUT_FORMAT, capacity, ChannelLayout::STEREO);
        resampler.run(&self.decoded, &mut output)?;

        let frames = output.samples();
        let bytes = &output.data(0)[..frames * SOURCE_CHANNELS * 4];
        let mut samples: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_ne_bytes(*b))
            .collect();

        let mut pts = pts;
        if let Some(target) = session.seek_target {
            let end = pts + frames as f64 / out_rate as f64;
            if end <= target {
                return Ok(());
            }
            if pts < target {
                let skip = (((target - pts) * out_rate as f64) as usize).min(frames);
                samples.drain(..skip * SOURCE_CHANNELS);
                pts += skip as f64 / out_rate as f64;
            }
        }
        if !samples.is_empty() {
            self.source.push(AudioChunk {
                pts,
                samples,
                pos: 0,
                rate: out_rate,
            });
        }
        Ok(())
    }
}
