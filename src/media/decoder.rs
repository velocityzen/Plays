//! Demuxing and decoding for one file or stream.
//!
//! Each player gets two threads. The demux thread reads compressed packets
//! ahead into one queue per stream: about a second for files, and up to
//! `STREAM_READ_AHEAD` for network streams so a slow or bursty connection
//! doesn't stall playback. The decode thread turns those packets into a small
//! queue of display-ready video frames (BGRA, wrapped in a gpui `RenderImage`)
//! and keeps the player's audio source topped up with resampled stereo `f32`.
//! It only decodes while an output queue has room, so memory stays bounded
//! however much is read ahead.

use std::collections::VecDeque;
use std::ffi::c_int;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
use parking_lot::{Condvar, Mutex};

use super::audio::{AudioChunk, AudioSource, SOURCE_CHANNELS};
use super::clock::Clock;
use super::hwaccel;
use super::source::Source;

/// Decoded video kept ahead of the playhead, by memory.
const VIDEO_QUEUE_BYTES: usize = 32 * 1024 * 1024;
const VIDEO_QUEUE_MIN_FRAMES: usize = 3;
const VIDEO_QUEUE_MAX_FRAMES: usize = 16;
/// Decoded audio kept ahead of the playhead.
const AUDIO_QUEUE_SECONDS: f64 = 0.75;
/// How far the demux thread reads ahead of the decoders.
const FILE_READ_AHEAD: ReadAhead = ReadAhead {
    seconds: 1.0,
    bytes: 16 * 1024 * 1024,
};
const STREAM_READ_AHEAD: ReadAhead = ReadAhead {
    seconds: 15.0,
    bytes: 48 * 1024 * 1024,
};
/// Hard caps on queued packets, reached only when one stream starves while
/// another one is far ahead (badly interleaved files).
const MAX_QUEUED_PACKETS: usize = 8192;
const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;
/// Network reads and connections give up after this long without data.
const NETWORK_TIMEOUT_MICROS: &str = "15000000";
/// Consecutive decoding errors after which a hardware decoder is replaced by
/// a software one.
const HARDWARE_ERROR_LIMIT: u32 = 8;
/// How long the decode thread sleeps when every queue is full.
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
    /// A network stream without an end (it can't be seeked).
    pub live: bool,
    pub has_video: bool,
    pub has_audio: bool,
    /// Display size of the video, after aspect-ratio correction and any cap.
    pub video_size: Option<(u32, u32)>,
    pub frame_rate: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    /// The hardware decoding API in use, once frames come out of it.
    pub hardware: Option<&'static str>,
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

/// Everything the UI thread and the decoder threads share for one player.
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
    /// Seconds of packets read ahead (`f64` bits), infinite once the demuxer
    /// reached the end. Network streams wait for some before playing.
    pub read_ahead: AtomicU64,
    /// Set when the player goes away; aborts network reads that are blocking.
    pub stop: AtomicBool,
}

impl Shared {
    pub fn read_ahead(&self) -> f64 {
        f64::from_bits(self.read_ahead.load(Ordering::Relaxed))
    }
}

#[derive(Clone, Copy)]
pub struct DecodeOptions {
    /// Cap on the shorter side of decoded video, in pixels.
    pub max_short_side: Option<u32>,
    /// Decode video on the GPU when possible.
    pub hardware: bool,
}

pub fn spawn(
    source: Source,
    shared: Arc<Shared>,
    options: DecodeOptions,
    commands: Receiver<Command>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name(format!("decode {}", source.name()))
        .spawn(move || {
            if let Err(err) = run(&source, &shared, options, &commands) {
                if shared.stop.load(Ordering::Relaxed) {
                    // Closed while opening; nobody is looking.
                    return;
                }
                log::error!("{source}: {err:#}");
                *shared.error.lock() = Some(format!("{err:#}"));
            }
        })
        .expect("spawning a decoder thread")
}

/// Per-seek decoding state handed to the stream decoders.
struct Session<'a> {
    shared: &'a Shared,
    name: &'a str,
    serial: u64,
    /// After a seek, output before this time is decoded but thrown away.
    seek_target: Option<f64>,
    /// Timestamp of the start of the media, subtracted from every frame.
    /// Taken from the first packet when the container doesn't say.
    start_offset: Option<f64>,
}

impl Session<'_> {
    fn start_offset(&self) -> f64 {
        self.start_offset.unwrap_or(0.0)
    }
}

fn open_input(source: &Source, shared: &Arc<Shared>) -> Result<ffmpeg::format::context::Input> {
    let mut options = ffmpeg::Dictionary::new();
    if source.is_network() {
        options.set("rw_timeout", NETWORK_TIMEOUT_MICROS);
        options.set("timeout", NETWORK_TIMEOUT_MICROS);
        options.set("reconnect", "1");
        options.set("reconnect_streamed", "1");
        options.set("reconnect_delay_max", "5");
        // Like most players: UDP rarely makes it through NAT and firewalls.
        options.set("rtsp_transport", "tcp");
    }
    let interrupt = shared.clone();
    ffmpeg::format::input_with_interrupt_and_dictionary(
        source.location(),
        move || interrupt.stop.load(Ordering::Relaxed),
        options,
    )
    .with_context(|| format!("could not open {source}"))
}

fn run(
    source: &Source,
    shared: &Arc<Shared>,
    options: DecodeOptions,
    commands: &Receiver<Command>,
) -> Result<()> {
    let input = open_input(source, shared)?;
    let name = source.name();

    let mut video: Option<VideoDecoder> = None;
    let mut cover: Option<VideoDecoder> = None;
    if let Some(stream) = input.streams().best(media::Type::Video) {
        let is_cover = stream.disposition().contains(Disposition::ATTACHED_PIC);
        let hardware = options.hardware && !is_cover;
        match VideoDecoder::new(&stream, options.max_short_side, hardware) {
            Ok(decoder) if is_cover => cover = Some(decoder),
            Ok(decoder) => video = Some(decoder),
            Err(err) => log::warn!("{source}: skipping video: {err:#}"),
        }
    }

    let mut audio: Option<AudioDecoder> = None;
    if let Some(stream) = input.streams().best(media::Type::Audio) {
        match &shared.audio {
            Some(output) => match AudioDecoder::new(&stream, output.clone()) {
                Ok(decoder) => audio = Some(decoder),
                Err(err) => log::warn!("{source}: skipping audio: {err:#}"),
            },
            None if video.is_none() => bail!("no audio output device is available"),
            None => log::warn!("{source}: no audio device, playing video only"),
        }
    }
    if video.is_none() && audio.is_none() {
        bail!("no playable audio or video streams");
    }

    let start_times: Vec<Option<f64>> = [
        video.as_ref().map(|v| v.index),
        audio.as_ref().map(|a| a.index),
    ]
    .into_iter()
    .flatten()
    .map(|index| {
        let stream = input.stream(index)?;
        (stream.start_time() != NO_PTS)
            .then(|| stream.start_time() as f64 * f64::from(stream.time_base()))
    })
    .collect();
    let start_offset = if start_times.iter().all(Option::is_some) {
        Some(
            start_times
                .into_iter()
                .flatten()
                .fold(f64::INFINITY, f64::min),
        )
    } else {
        None
    };

    let duration = (input.duration() > 0).then(|| input.duration() as f64 / AV_TIME_BASE);
    let info = MediaInfo {
        duration,
        live: source.is_network() && duration.is_none(),
        has_video: video.is_some(),
        has_audio: audio.is_some(),
        video_size: video.as_ref().map(|v| v.display_size()),
        frame_rate: video.as_ref().map(|v| 1.0 / v.frame_duration),
        video_codec: video.as_ref().map(|v| v.codec_name()),
        audio_codec: audio.as_ref().map(|a| a.codec_name()),
        hardware: None,
        title: input.metadata().get("title").map(str::to_owned),
    };
    *shared.info.lock() = Some(info);

    let mut session = Session {
        shared,
        name: &name,
        serial: shared.serial.load(Ordering::Acquire),
        seek_target: None,
        start_offset,
    };
    reset_queues(shared, session.serial);

    let demuxer = Arc::new(Demuxer {
        packets: Mutex::new(Packets {
            video: video
                .as_ref()
                .map(|v| PacketQueue::new(v.index, v.time_base)),
            audio: audio
                .as_ref()
                .map(|a| PacketQueue::new(a.index, a.time_base)),
            cover_index: cover.as_ref().map(|c| c.index),
            cover: None,
            cover_seen: false,
            bytes: 0,
            serial: session.serial,
            eof: false,
            seek: None,
            stopped: false,
        }),
        wake: Condvar::new(),
        limits: if source.is_network() {
            STREAM_READ_AHEAD
        } else {
            FILE_READ_AHEAD
        },
        shared: shared.clone(),
    });
    {
        let demuxer = demuxer.clone();
        let name = name.clone();
        thread::Builder::new()
            .name(format!("demux {name}"))
            .spawn(move || demux(input, &demuxer, &name))
            .context("starting the demux thread")?;
    }
    // Stops the demux thread however this function returns.
    let _demuxer_guard = StopOnDrop(demuxer.clone());

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
            let target = ((time + session.start_offset()).max(0.0) * AV_TIME_BASE) as i64;
            demuxer.seek(target, serial);
            if let Some(v) = &mut video {
                v.reset();
            }
            if let Some(a) = &mut audio {
                a.reset();
            }
            session.serial = serial;
            session.seek_target = Some(time);
            reset_queues(shared, serial);
        }

        let mut progressed = false;

        // Decode a packet for each output queue that has room.
        let video_room = video.as_ref().is_some_and(|v| {
            !v.finished && shared.video.lock().frames.len() < v.max_queued_frames()
        });
        if video_room && let Some(v) = &mut video {
            match demuxer.pop(true, session.serial) {
                Next::Packet(packet) => {
                    session.start_offset = session
                        .start_offset
                        .or_else(|| packet_time(&packet, v.time_base));
                    v.decode(&packet, &session);
                    progressed = true;
                }
                Next::End => {
                    v.finish(&session);
                    progressed = true;
                }
                Next::Wait => {}
            }
        }
        let audio_room = audio.as_ref().is_some_and(|a| !a.finished && a.has_room());
        if audio_room && let Some(a) = &mut audio {
            match demuxer.pop(false, session.serial) {
                Next::Packet(packet) => {
                    session.start_offset = session
                        .start_offset
                        .or_else(|| packet_time(&packet, a.time_base));
                    a.decode(&packet, &session);
                    progressed = true;
                }
                Next::End => {
                    a.finish(&session);
                    progressed = true;
                }
                Next::Wait => {}
            }
        }
        if let Some(c) = &mut cover
            && let Some(packet) = demuxer.take_cover()
        {
            let mut slot = shared.cover.lock();
            if slot.is_none() {
                *slot = c.decode_still(&packet);
            }
        }

        let done =
            video.as_ref().is_none_or(|v| v.finished) && audio.as_ref().is_none_or(|a| a.finished);
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

fn packet_time(packet: &Packet, time_base: f64) -> Option<f64> {
    packet
        .pts()
        .or(packet.dts())
        .map(|ts| ts as f64 * time_base)
}

// ---------------------------------------------------------------------------
// Demuxing

#[derive(Clone, Copy)]
struct ReadAhead {
    seconds: f64,
    bytes: usize,
}

/// Packets read ahead for one stream.
struct PacketQueue {
    index: usize,
    time_base: f64,
    packets: VecDeque<Packet>,
}

impl PacketQueue {
    fn new(index: usize, time_base: f64) -> Self {
        Self {
            index,
            time_base,
            packets: VecDeque::new(),
        }
    }

    /// Seconds between the first and the last queued packet.
    fn span(&self) -> f64 {
        let time = |p: &Packet| p.dts().or(p.pts());
        match (
            self.packets.front().and_then(time),
            self.packets.back().and_then(time),
        ) {
            (Some(first), Some(last)) => (last - first).max(0) as f64 * self.time_base,
            // No timestamps: guess from the count.
            _ => self.packets.len() as f64 / 50.0,
        }
    }
}

struct Packets {
    video: Option<PacketQueue>,
    audio: Option<PacketQueue>,
    cover_index: Option<usize>,
    cover: Option<Packet>,
    cover_seen: bool,
    bytes: usize,
    /// The seek generation the queued packets belong to.
    serial: u64,
    eof: bool,
    /// A seek (target in `AV_TIME_BASE` units, serial) for the demux thread.
    seek: Option<(i64, u64)>,
    stopped: bool,
}

impl Packets {
    fn queues(&self) -> impl Iterator<Item = &PacketQueue> {
        self.video.iter().chain(self.audio.iter())
    }

    fn wants_more(&self, limits: ReadAhead) -> bool {
        let count: usize = self.queues().map(|q| q.packets.len()).sum();
        if self.eof || count >= MAX_QUEUED_PACKETS {
            return false;
        }
        let starving = self.queues().any(|q| q.packets.is_empty());
        let short = self.queues().any(|q| q.span() < limits.seconds);
        (starving && self.bytes < MAX_QUEUED_BYTES) || (short && self.bytes < limits.bytes)
    }

    /// Seconds that can be played from queued packets alone.
    fn read_ahead(&self) -> f64 {
        if self.eof {
            return f64::INFINITY;
        }
        self.queues()
            .map(PacketQueue::span)
            .fold(f64::INFINITY, f64::min)
    }

    fn push(&mut self, packet: Packet) {
        let index = packet.stream();
        let queue = [&mut self.video, &mut self.audio]
            .into_iter()
            .flatten()
            .find(|q| q.index == index);
        if let Some(queue) = queue {
            self.bytes += packet.size();
            queue.packets.push_back(packet);
        } else if self.cover_index == Some(index) && !self.cover_seen {
            self.cover_seen = true;
            self.cover = Some(packet);
        }
    }

    fn clear(&mut self) {
        for queue in [&mut self.video, &mut self.audio].into_iter().flatten() {
            queue.packets.clear();
        }
        self.bytes = 0;
        self.eof = false;
    }
}

enum Next {
    Packet(Packet),
    /// No more packets until the next seek.
    End,
    /// Nothing queued yet.
    Wait,
}

/// The packet queues between the demux thread and the decode thread.
struct Demuxer {
    packets: Mutex<Packets>,
    /// Wakes the demux thread when there's room, a seek or a stop.
    wake: Condvar,
    limits: ReadAhead,
    shared: Arc<Shared>,
}

impl Demuxer {
    fn publish(&self, packets: &Packets) {
        self.shared
            .read_ahead
            .store(packets.read_ahead().to_bits(), Ordering::Relaxed);
    }

    fn pop(&self, video: bool, serial: u64) -> Next {
        let mut packets = self.packets.lock();
        if packets.serial != serial {
            // The demux thread hasn't got to the seek yet.
            return Next::Wait;
        }
        let queue = if video {
            packets.video.as_mut()
        } else {
            packets.audio.as_mut()
        };
        match queue.and_then(|q| q.packets.pop_front()) {
            Some(packet) => {
                packets.bytes -= packet.size();
                self.publish(&packets);
                self.wake.notify_one();
                Next::Packet(packet)
            }
            None if packets.eof => Next::End,
            None => Next::Wait,
        }
    }

    fn take_cover(&self) -> Option<Packet> {
        self.packets.lock().cover.take()
    }

    fn seek(&self, target: i64, serial: u64) {
        let mut packets = self.packets.lock();
        packets.clear();
        packets.seek = Some((target, serial));
        self.publish(&packets);
        self.wake.notify_one();
    }

    fn stop(&self) {
        self.packets.lock().stopped = true;
        self.wake.notify_one();
    }
}

struct StopOnDrop(Arc<Demuxer>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// The demux thread: reads packets until the queues are full enough, and
/// carries out seeks.
fn demux(mut input: ffmpeg::format::context::Input, demuxer: &Demuxer, name: &str) {
    let stopping = || demuxer.shared.stop.load(Ordering::Relaxed);
    loop {
        let seek = {
            let mut packets = demuxer.packets.lock();
            loop {
                if packets.stopped || stopping() {
                    return;
                }
                if packets.seek.is_some() || packets.wants_more(demuxer.limits) {
                    break;
                }
                demuxer.wake.wait(&mut packets);
            }
            packets.seek
        };

        if let Some((target, serial)) = seek {
            if let Err(err) = input.seek(target, ..target) {
                log::warn!(
                    "{name}: seek to {:.2}s failed: {err}",
                    target as f64 / AV_TIME_BASE
                );
            }
            let mut packets = demuxer.packets.lock();
            // A newer seek may have come in meanwhile; it runs next time round.
            if packets.seek == Some((target, serial)) {
                packets.seek = None;
                packets.serial = serial;
            }
            continue;
        }

        let mut packet = Packet::empty();
        let result = packet.read(&mut input);
        let mut packets = demuxer.packets.lock();
        if packets.seek.is_some() {
            // Read from before the seek position.
            continue;
        }
        match result {
            Ok(()) => packets.push(packet),
            Err(ffmpeg::Error::Eof) => packets.eof = true,
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                drop(packets);
                thread::sleep(IDLE_WAIT);
                continue;
            }
            Err(_) if stopping() => return,
            Err(err) => {
                log::warn!("{name}: read error, treating it as the end: {err}");
                packets.eof = true;
            }
        }
        demuxer.publish(&packets);
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
    /// Kept to reopen the decoder in software if the hardware one fails.
    parameters: codec::Parameters,
    time_base: f64,
    frame_duration: f64,
    max_short_side: Option<u32>,
    scaler: Option<Scaler>,
    decoded: frame::Video,
    /// Hardware frames copied back from the GPU.
    downloaded: frame::Video,
    /// The hardware decoding API attached to the decoder, if any.
    hardware: Option<&'static str>,
    /// Whether the last frame came from the hardware decoder, as last reported.
    reported: Option<bool>,
    errors: u32,
    next_pts: f64,
    finished: bool,
}

/// Open a decoder for the stream, on the GPU when asked and possible.
fn open_video(
    parameters: &codec::Parameters,
    hardware: bool,
) -> Result<(ffmpeg::decoder::Video, Option<&'static str>)> {
    let mut context = codec::context::Context::from_parameters(parameters.clone())?;
    let codec = ffmpeg::decoder::find(context.id())
        .ok_or_else(|| anyhow!("there is no decoder for {}", codec_name(parameters.id())))?;
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
    let api = if hardware {
        hwaccel::attach(&mut context, &codec)
    } else {
        None
    };
    match (
        context.decoder().open_as(codec).and_then(|d| d.video()),
        api,
    ) {
        (Ok(decoder), _) => Ok((decoder, api)),
        (Err(err), Some(api)) => {
            log::info!("{api} couldn't open this video ({err}); decoding in software");
            open_video(parameters, false)
        }
        (Err(err), None) => Err(err).context("opening the video decoder"),
    }
}

impl VideoDecoder {
    fn new(
        stream: &ffmpeg::format::stream::Stream,
        max_short_side: Option<u32>,
        hardware: bool,
    ) -> Result<Self> {
        let parameters = stream.parameters().clone();
        let (decoder, hardware) = open_video(&parameters, hardware)?;
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
            parameters,
            time_base: f64::from(stream.time_base()),
            frame_duration,
            max_short_side,
            scaler: None,
            decoded: frame::Video::empty(),
            downloaded: frame::Video::empty(),
            hardware,
            reported: None,
            errors: 0,
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
            self.failed(session);
        }
        self.drain(session);
    }

    fn finish(&mut self, session: &Session) {
        let _ = self.decoder.send_eof();
        self.drain(session);
        self.finished = true;
    }

    /// Count a decoding error. A hardware decoder that keeps failing (a driver
    /// that accepts a stream it can't actually decode) is swapped for a
    /// software one; it picks up again at the next keyframe.
    fn failed(&mut self, session: &Session) {
        let Some(api) = self.hardware else {
            return;
        };
        self.errors += 1;
        if self.errors < HARDWARE_ERROR_LIMIT {
            return;
        }
        log::warn!(
            "{}: {api} keeps failing on this video; decoding in software",
            session.name
        );
        match open_video(&self.parameters, false) {
            Ok((decoder, _)) => {
                self.decoder = decoder;
                self.hardware = None;
                self.errors = 0;
            }
            Err(err) => log::warn!("{}: {err:#}", session.name),
        }
    }

    /// Note in the media info whether frames come from the GPU.
    fn report(&mut self, hardware: bool, session: &Session) {
        if self.reported == Some(hardware) {
            return;
        }
        self.reported = Some(hardware);
        let api = self.hardware.filter(|_| hardware);
        match (api, self.hardware) {
            (Some(api), _) => log::info!("{}: decoding video with {api}", session.name),
            (None, Some(api)) => log::info!(
                "{}: {api} can't decode this video; decoding in software",
                session.name
            ),
            (None, None) => {}
        }
        if let Some(info) = session.shared.info.lock().as_mut() {
            info.hardware = api;
        }
    }

    fn drain(&mut self, session: &Session) {
        loop {
            match self.decoder.receive_frame(&mut self.decoded) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => break,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                    break;
                }
                Err(err) => {
                    log::debug!("video decode error: {err}");
                    self.failed(session);
                    break;
                }
            }
            self.errors = 0;
            let on_gpu = hwaccel::is_hardware_frame(&self.decoded);
            self.report(on_gpu, session);

            let pts = self
                .decoded
                .timestamp()
                .map(|ts| ts as f64 * self.time_base - session.start_offset())
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
            let frame = if on_gpu {
                if let Err(err) = hwaccel::download(&self.decoded, &mut self.downloaded) {
                    log::warn!("{err:#}");
                    continue;
                }
                &self.downloaded
            } else {
                &self.decoded
            };
            match convert(&mut self.scaler, self.max_short_side, frame) {
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
        convert(&mut self.scaler, self.max_short_side, &self.decoded)
            .inspect_err(|err| log::warn!("cover art conversion failed: {err:#}"))
            .ok()
    }
}

/// Convert a decoded frame to a BGRA image gpui can draw.
fn convert(
    scaler: &mut Option<Scaler>,
    max_short_side: Option<u32>,
    frame: &frame::Video,
) -> Result<Arc<RenderImage>> {
    let key = (frame.format(), frame.width(), frame.height());
    if scaler.as_ref().is_none_or(|s| s.key != key) {
        let (width, height) = output_size(
            frame.width(),
            frame.height(),
            frame.aspect_ratio(),
            max_short_side,
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
        *scaler = Some(Scaler {
            context,
            key,
            width,
            height,
            output: frame::Video::empty(),
        });
    }
    let scaler = scaler.as_mut().expect("scaler was just created");
    scaler.context.run(frame, &mut scaler.output)?;

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
            self.source.queue.lock().push(AudioChunk {
                pts,
                samples,
                pos: 0,
            });
        }
        Ok(())
    }
}
