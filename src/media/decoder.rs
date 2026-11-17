//! Demuxing and decoding for one media file, on its own thread.
//!
//! The thread keeps a small queue of display-ready video frames (BGRA, wrapped
//! in a gpui `RenderImage`) and keeps the player's audio source topped up with
//! resampled stereo `f32`. Packets are read only when a decoder needs more input,
//! and packets for a stream whose output queue is full are parked, so badly
//! interleaved files never balloon the decoded-frame queue.

use std::collections::VecDeque;
use std::ffi::c_int;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use ffmpeg::format::stream::Disposition;
use ffmpeg::software::{resampling, scaling};
use ffmpeg::util::{channel_layout::ChannelLayout, color, format::Pixel, format::Sample};
use ffmpeg::{Packet, Rational, codec, frame, media};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;
use parking_lot::Mutex;

use super::audio::{AudioChunk, AudioSource, SOURCE_CHANNELS};
use super::clock::Clock;

/// Decoded video kept ahead of the playhead, by memory.
const VIDEO_QUEUE_BYTES: usize = 32 * 1024 * 1024;
const VIDEO_QUEUE_MIN_FRAMES: usize = 3;
const VIDEO_QUEUE_MAX_FRAMES: usize = 16;
/// Decoded audio kept ahead of the playhead.
const AUDIO_QUEUE_SECONDS: f64 = 0.75;
/// Parked compressed packets, across all streams.
const MAX_PARKED_PACKETS: usize = 4096;
const MAX_PARKED_BYTES: usize = 64 * 1024 * 1024;
/// How long the thread sleeps when every queue is full.
const IDLE_WAIT: Duration = Duration::from_millis(4);
const DONE_WAIT: Duration = Duration::from_millis(100);
const AV_TIME_BASE: f64 = 1_000_000.0;
const NO_PTS: i64 = i64::MIN;

pub enum Command {
    Seek { time: f64, serial: u64 },
    Stop,
}

#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub duration: Option<f64>,
    pub has_video: bool,
    pub has_audio: bool,
    /// Display size of the video, after aspect-ratio correction and any cap.
    pub video_size: Option<(u32, u32)>,
    pub frame_rate: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub title: Option<String>,
}

pub struct VideoFrame {
    pub pts: f64,
    pub serial: u64,
    pub image: Arc<RenderImage>,
}

#[derive(Default)]
pub struct VideoQueue {
    pub frames: VecDeque<VideoFrame>,
    pub serial: u64,
}

/// Everything the UI thread and the decoder thread share for one player.
pub struct Shared {
    pub clock: Arc<Clock>,
    /// The seek generation the UI wants; frames and samples from older
    /// generations are ignored.
    pub serial: Arc<AtomicU64>,
    pub info: Mutex<Option<MediaInfo>>,
    pub error: Mutex<Option<String>>,
    pub video: Mutex<VideoQueue>,
    pub audio: Option<Arc<AudioSource>>,
    /// Embedded cover art (for example in an MP3 or M4A).
    pub cover: Mutex<Option<Arc<RenderImage>>>,
    /// Set to the current serial once every frame and sample for it has been queued.
    pub eof_serial: AtomicU64,
}

#[derive(Clone, Copy)]
pub struct DecodeOptions {
    /// Cap on the shorter side of decoded video, in pixels.
    pub max_short_side: Option<u32>,
}

pub fn spawn(
    path: PathBuf,
    shared: Arc<Shared>,
    options: DecodeOptions,
    commands: Receiver<Command>,
) -> JoinHandle<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    thread::Builder::new()
        .name(format!("decode {name}"))
        .spawn(move || {
            if let Err(err) = run(&path, &shared, options, &commands) {
                log::error!("{}: {err:#}", path.display());
                *shared.error.lock() = Some(format!("{err:#}"));
            }
        })
        .expect("spawning a decoder thread")
}

/// Per-seek decoding state handed to the stream decoders.
struct Session<'a> {
    shared: &'a Shared,
    serial: u64,
    /// After a seek, output before this time is decoded but thrown away.
    seek_target: Option<f64>,
    start_offset: f64,
}

fn run(
    path: &Path,
    shared: &Shared,
    options: DecodeOptions,
    commands: &Receiver<Command>,
) -> Result<()> {
    let mut input = ffmpeg::format::input(path)
        .with_context(|| format!("could not open {}", path.display()))?;

    let mut video: Option<VideoDecoder> = None;
    let mut cover: Option<VideoDecoder> = None;
    if let Some(stream) = input.streams().best(media::Type::Video) {
        let is_cover = stream.disposition().contains(Disposition::ATTACHED_PIC);
        match VideoDecoder::new(&stream, options.max_short_side) {
            Ok(decoder) if is_cover => cover = Some(decoder),
            Ok(decoder) => video = Some(decoder),
            Err(err) => log::warn!("{}: skipping video: {err:#}", path.display()),
        }
    }

    let mut audio: Option<AudioDecoder> = None;
    if let Some(stream) = input.streams().best(media::Type::Audio) {
        match &shared.audio {
            Some(source) => match AudioDecoder::new(&stream, source.clone()) {
                Ok(decoder) => audio = Some(decoder),
                Err(err) => log::warn!("{}: skipping audio: {err:#}", path.display()),
            },
            None if video.is_none() => bail!("no audio output device is available"),
            None => log::warn!("{}: no audio device, playing video only", path.display()),
        }
    }
    if video.is_none() && audio.is_none() {
        bail!("no playable audio or video streams");
    }

    let start_offset = [
        video.as_ref().map(|v| v.index),
        audio.as_ref().map(|a| a.index),
    ]
    .into_iter()
    .flatten()
    .filter_map(|index| {
        let stream = input.stream(index)?;
        (stream.start_time() != NO_PTS)
            .then(|| stream.start_time() as f64 * f64::from(stream.time_base()))
    })
    .reduce(f64::min)
    .unwrap_or(0.0);

    let duration = (input.duration() > 0).then(|| input.duration() as f64 / AV_TIME_BASE);
    let info = MediaInfo {
        duration,
        has_video: video.is_some(),
        has_audio: audio.is_some(),
        video_size: video.as_ref().map(|v| v.display_size()),
        frame_rate: video.as_ref().map(|v| 1.0 / v.frame_duration),
        video_codec: video.as_ref().map(|v| v.codec_name()),
        audio_codec: audio.as_ref().map(|a| a.codec_name()),
        title: input.metadata().get("title").map(str::to_owned),
    };
    *shared.info.lock() = Some(info);

    let mut session = Session {
        shared,
        serial: shared.serial.load(Ordering::Acquire),
        seek_target: None,
        start_offset,
    };
    reset_queues(shared, session.serial);

    let mut video_packets = VecDeque::<Packet>::new();
    let mut audio_packets = VecDeque::<Packet>::new();
    let mut parked_bytes = 0usize;
    let mut demux_eof = false;
    let mut wait: Option<Duration> = None;

    loop {
        // Commands. Seeks are coalesced so dragging a slider only seeks to the latest spot.
        let mut seek = None;
        let first = match wait {
            Some(timeout) => match commands.recv_timeout(timeout) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            },
            None => match commands.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return Ok(()),
            },
        };
        if let Some(mut command) = first {
            loop {
                match command {
                    Command::Stop => return Ok(()),
                    Command::Seek { time, serial } => seek = Some((time, serial)),
                }
                command = match commands.try_recv() {
                    Ok(next) => next,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                };
            }
        }

        if let Some((time, serial)) = seek {
            let target = ((time + session.start_offset).max(0.0) * AV_TIME_BASE) as i64;
            if let Err(err) = input.seek(target, ..target) {
                log::warn!("{}: seek to {time:.2}s failed: {err}", path.display());
            }
            if let Some(v) = &mut video {
                v.reset();
            }
            if let Some(a) = &mut audio {
                a.reset();
            }
            video_packets.clear();
            audio_packets.clear();
            parked_bytes = 0;
            demux_eof = false;
            session.serial = serial;
            session.seek_target = Some(time);
            reset_queues(shared, serial);
        }

        let mut progressed = false;

        // Decode parked packets into any output queue that has room.
        let video_room = video
            .as_ref()
            .is_some_and(|v| shared.video.lock().frames.len() < v.max_queued_frames());
        if video_room && let Some(v) = &mut video {
            if let Some(packet) = video_packets.pop_front() {
                parked_bytes -= packet.size();
                v.decode(&packet, &session);
                progressed = true;
            } else if demux_eof && !v.finished {
                v.finish(&session);
                progressed = true;
            }
        }
        let audio_room = audio.as_ref().is_some_and(|a| a.has_room());
        if audio_room && let Some(a) = &mut audio {
            if let Some(packet) = audio_packets.pop_front() {
                parked_bytes -= packet.size();
                a.decode(&packet, &session);
                progressed = true;
            } else if demux_eof && !a.finished {
                a.finish(&session);
                progressed = true;
            }
        }

        // Read another packet only when a decoder with room has run dry.
        let video_hungry = video_room && video_packets.is_empty();
        let audio_hungry = audio_room && audio_packets.is_empty();
        let parked = video_packets.len() + audio_packets.len();
        if !demux_eof
            && (video_hungry || audio_hungry)
            && parked < MAX_PARKED_PACKETS
            && parked_bytes < MAX_PARKED_BYTES
        {
            let mut packet = Packet::empty();
            match packet.read(&mut input) {
                Ok(()) => {
                    let index = packet.stream();
                    if video.as_ref().is_some_and(|v| v.index == index) {
                        parked_bytes += packet.size();
                        video_packets.push_back(packet);
                    } else if audio.as_ref().is_some_and(|a| a.index == index) {
                        parked_bytes += packet.size();
                        audio_packets.push_back(packet);
                    } else if let Some(c) = &mut cover
                        && c.index == index
                    {
                        let mut slot = shared.cover.lock();
                        if slot.is_none() {
                            *slot = c.decode_still(&packet);
                        }
                    }
                }
                Err(ffmpeg::Error::Eof) => demux_eof = true,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {}
                Err(err) => {
                    log::warn!(
                        "{}: read error, treating as end of file: {err}",
                        path.display()
                    );
                    demux_eof = true;
                }
            }
            progressed = true;
        }

        let done = demux_eof
            && video_packets.is_empty()
            && audio_packets.is_empty()
            && video.as_ref().is_none_or(|v| v.finished)
            && audio.as_ref().is_none_or(|a| a.finished);
        if done {
            shared.eof_serial.store(session.serial, Ordering::Release);
        }

        wait = match (progressed, done) {
            (true, _) => None,
            (false, true) => Some(DONE_WAIT),
            (false, false) => Some(IDLE_WAIT),
        };
    }
}

fn reset_queues(shared: &Shared, serial: u64) {
    {
        let mut video = shared.video.lock();
        video.frames.clear();
        video.serial = serial;
    }
    if let Some(audio) = &shared.audio {
        audio.queue.lock().clear(serial);
    }
}

fn rational(r: Rational) -> Option<f64> {
    (r.numerator() > 0 && r.denominator() > 0).then(|| f64::from(r))
}

fn codec_name(id: codec::Id) -> String {
    format!("{id:?}").to_lowercase()
}

// ---------------------------------------------------------------------------
// Video

struct Scaler {
    context: scaling::Context,
    key: (Pixel, u32, u32),
    width: u32,
    height: u32,
    output: frame::Video,
}

struct VideoDecoder {
    index: usize,
    decoder: ffmpeg::decoder::Video,
    time_base: f64,
    frame_duration: f64,
    max_short_side: Option<u32>,
    scaler: Option<Scaler>,
    decoded: frame::Video,
    next_pts: f64,
    finished: bool,
}

impl VideoDecoder {
    fn new(stream: &ffmpeg::format::stream::Stream, max_short_side: Option<u32>) -> Result<Self> {
        let mut context = codec::context::Context::from_parameters(stream.parameters())?;
        // Several files decode at once, so a few threads each is plenty and
        // keeps memory down (FFmpeg's default is one per core per decoder).
        let threads = thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 6));
        // `safe` only exists with older FFmpeg versions, hence the struct update.
        #[allow(clippy::needless_update)]
        let threading = codec::threading::Config {
            kind: codec::threading::Type::Frame,
            count: threads,
            ..Default::default()
        };
        context.set_threading(threading);
        let decoder = context
            .decoder()
            .video()
            .context("opening the video decoder")?;
        if decoder.width() == 0 || decoder.height() == 0 {
            bail!("video stream has no dimensions");
        }
        let frame_duration = rational(stream.avg_frame_rate())
            .or_else(|| rational(stream.rate()))
            .filter(|fps| (1.0..=480.0).contains(fps))
            .map(|fps| 1.0 / fps)
            .unwrap_or(1.0 / 25.0);
        Ok(Self {
            index: stream.index(),
            decoder,
            time_base: f64::from(stream.time_base()),
            frame_duration,
            max_short_side,
            scaler: None,
            decoded: frame::Video::empty(),
            next_pts: 0.0,
            finished: false,
        })
    }

    fn codec_name(&self) -> String {
        codec_name(self.decoder.id())
    }

    fn display_size(&self) -> (u32, u32) {
        output_size(
            self.decoder.width(),
            self.decoder.height(),
            self.decoder.aspect_ratio(),
            self.max_short_side,
        )
    }

    /// Keep roughly `VIDEO_QUEUE_BYTES` of decoded frames ready.
    fn max_queued_frames(&self) -> usize {
        let (w, h) = self.display_size();
        let frame_bytes = (w as usize * h as usize * 4).max(1);
        (VIDEO_QUEUE_BYTES / frame_bytes).clamp(VIDEO_QUEUE_MIN_FRAMES, VIDEO_QUEUE_MAX_FRAMES)
    }

    fn reset(&mut self) {
        self.decoder.flush();
        self.finished = false;
    }

    fn decode(&mut self, packet: &Packet, session: &Session) {
        if let Err(err) = self.decoder.send_packet(packet) {
            log::debug!("video decode error: {err}");
        }
        self.drain(session);
    }

    fn finish(&mut self, session: &Session) {
        let _ = self.decoder.send_eof();
        self.drain(session);
        self.finished = true;
    }

    fn drain(&mut self, session: &Session) {
        while self.decoder.receive_frame(&mut self.decoded).is_ok() {
            let pts = self
                .decoded
                .timestamp()
                .map(|ts| ts as f64 * self.time_base - session.start_offset)
                .unwrap_or(self.next_pts);
            self.next_pts = pts + self.frame_duration;

            // Skip conversion for frames that end before the seek target.
            if session
                .seek_target
                .is_some_and(|target| pts + self.frame_duration <= target)
            {
                continue;
            }
            // Skip frames whose time has already passed while something else is
            // still waiting to be shown (the UI isn't keeping up, e.g. the window
            // is hidden). With nothing queued, show it anyway so a decoder that
            // is slower than real time still puts pictures on screen.
            let late = pts + self.frame_duration < session.shared.clock.now();
            if late && !session.shared.video.lock().frames.is_empty() {
                continue;
            }
            match self.convert() {
                Ok(image) => session.shared.video.lock().frames.push_back(VideoFrame {
                    pts,
                    serial: session.serial,
                    image,
                }),
                Err(err) => log::warn!("video conversion failed: {err:#}"),
            }
        }
    }

    /// Decode a single picture, used for embedded cover art.
    fn decode_still(&mut self, packet: &Packet) -> Option<Arc<RenderImage>> {
        self.decoder.send_packet(packet).ok()?;
        let _ = self.decoder.send_eof();
        self.decoder.receive_frame(&mut self.decoded).ok()?;
        self.convert()
            .inspect_err(|err| log::warn!("cover art conversion failed: {err:#}"))
            .ok()
    }

    /// Convert the last decoded frame to a BGRA image gpui can draw.
    fn convert(&mut self) -> Result<Arc<RenderImage>> {
        let frame = &self.decoded;
        let key = (frame.format(), frame.width(), frame.height());
        if self.scaler.as_ref().is_none_or(|s| s.key != key) {
            let (width, height) = output_size(
                frame.width(),
                frame.height(),
                frame.aspect_ratio(),
                self.max_short_side,
            );
            let mut context = scaling::Context::get(
                key.0,
                key.1,
                key.2,
                Pixel::BGRA,
                width,
                height,
                scaling::Flags::BILINEAR,
            )
            .context("creating the pixel converter")?;
            set_colorspace(&mut context, frame);
            self.scaler = Some(Scaler {
                context,
                key,
                width,
                height,
                output: frame::Video::empty(),
            });
        }
        let scaler = self.scaler.as_mut().expect("scaler was just created");
        scaler.context.run(&self.decoded, &mut scaler.output)?;

        let (width, height) = (scaler.width, scaler.height);
        let row = width as usize * 4;
        let stride = scaler.output.stride(0);
        let data = scaler.output.data(0);
        let mut pixels = Vec::with_capacity(row * height as usize);
        for y in 0..height as usize {
            pixels.extend_from_slice(&data[y * stride..y * stride + row]);
        }
        // gpui reads image bytes as BGRA, which is exactly what we converted to.
        let buffer = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow!("frame buffer has the wrong size"))?;
        Ok(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
    }
}

/// Size to convert a frame to: corrects non-square pixels and applies the
/// resolution cap to the shorter side, keeping both sides even.
pub(crate) fn output_size(
    width: u32,
    height: u32,
    sar: Rational,
    max_short_side: Option<u32>,
) -> (u32, u32) {
    let sar = rational(sar).unwrap_or(1.0);
    let mut w = width as f64 * sar;
    let mut h = height as f64;
    if let Some(max) = max_short_side {
        let short = w.min(h);
        if short > max as f64 {
            let scale = max as f64 / short;
            w *= scale;
            h *= scale;
        }
    }
    let even = |v: f64| ((v.round() as u32) & !1).max(2);
    (even(w), even(h))
}

/// Use the right YUV matrix and range so colors match other players.
fn set_colorspace(context: &mut scaling::Context, frame: &frame::Video) {
    use ffmpeg::ffi;
    let matrix = match frame.color_space() {
        color::Space::BT709 => ffi::SWS_CS_ITU709,
        color::Space::FCC => ffi::SWS_CS_FCC,
        color::Space::SMPTE240M => ffi::SWS_CS_SMPTE240M,
        color::Space::BT2020NCL | color::Space::BT2020CL => ffi::SWS_CS_BT2020,
        color::Space::Unspecified if frame.height() >= 720 => ffi::SWS_CS_ITU709,
        _ => ffi::SWS_CS_DEFAULT,
    };
    let full_range = matches!(frame.color_range(), color::Range::JPEG) as c_int;
    // SAFETY: the context pointer is valid for the lifetime of `context`, and
    // sws_getCoefficients returns pointers to static tables.
    unsafe {
        ffi::sws_setColorspaceDetails(
            context.as_mut_ptr(),
            ffi::sws_getCoefficients(matrix),
            full_range,
            ffi::sws_getCoefficients(ffi::SWS_CS_DEFAULT),
            1,
            0,
            1 << 16,
            1 << 16,
        );
    }
}

// ---------------------------------------------------------------------------
// Audio

struct AudioDecoder {
    index: usize,
    decoder: ffmpeg::decoder::Audio,
    source: Arc<AudioSource>,
    time_base: f64,
    resampler: Option<resampling::Context>,
    decoded: frame::Audio,
    next_pts: Option<f64>,
    finished: bool,
}

const OUTPUT_FORMAT: Sample = Sample::F32(ffmpeg::util::format::sample::Type::Packed);

impl AudioDecoder {
    fn new(stream: &ffmpeg::format::stream::Stream, source: Arc<AudioSource>) -> Result<Self> {
        let context = codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context
            .decoder()
            .audio()
            .context("opening the audio decoder")?;
        Ok(Self {
            index: stream.index(),
            decoder,
            source,
            time_base: f64::from(stream.time_base()),
            resampler: None,
            decoded: frame::Audio::empty(),
            next_pts: None,
            finished: false,
        })
    }

    fn codec_name(&self) -> String {
        codec_name(self.decoder.id())
    }

    fn output_rate(&self) -> u32 {
        self.source.sample_rate
    }

    fn has_room(&self) -> bool {
        let buffered = self.source.queue.lock().buffered_frames;
        (buffered as f64) < AUDIO_QUEUE_SECONDS * self.output_rate() as f64
    }

    fn reset(&mut self) {
        self.decoder.flush();
        self.resampler = None;
        self.next_pts = None;
        self.finished = false;
    }

    fn decode(&mut self, packet: &Packet, session: &Session) {
        if let Err(err) = self.decoder.send_packet(packet) {
            log::debug!("audio decode error: {err}");
        }
        self.drain(session);
    }

    fn finish(&mut self, session: &Session) {
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
        if self.resampler.as_ref().is_none_or(|r| *r.input() != input) {
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
            .map(|ts| ts as f64 * self.time_base - session.start_offset)
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
            self.source.queue.lock().push(AudioChunk {
                pts,
                samples,
                pos: 0,
            });
        }
        Ok(())
    }
}
