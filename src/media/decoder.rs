//! Demuxing and decoding for one file or stream.
//!
//! Each player gets two threads. The demux thread reads compressed packets
//! ahead into one queue per stream: about a second for files, and up to
//! `STREAM_READ_AHEAD` for network streams so a slow or bursty connection
//! doesn't stall playback. The decode thread turns those packets into a small
//! queue of display-ready video frames and keeps the player's audio source
//! topped up with resampled stereo `f32`. Frames are BGRA images, except on
//! macOS with hardware decoding, where they stay on the GPU (`videotoolbox`),
//! and on Linux, where most are handed over as their Y'CbCr planes for the
//! GPU to convert (`yuv`). HDR frames that become images are tone mapped for
//! an SDR screen (`hdr`), Dolby Vision ones reshaped first (`dovi`).
//! Subtitles are decoded into cues as soon as they are read (`subtitles`),
//! or, from a subtitle file beside the video, a little ahead of the playhead
//! (`external`). The other subtitle tracks' packets are kept for a few
//! seconds, so one picked shows without a seek that would drop the frames
//! queued.
//! It only decodes while an output queue has room, so memory stays bounded
//! however much is read ahead. One video, one audio and one subtitle track
//! play at a time; another can be picked while playing.

use std::collections::VecDeque;
use std::ffi::c_int;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use ffmpeg::Discard;
use ffmpeg::format::stream::Disposition;
use ffmpeg::software::{resampling, scaling};
use ffmpeg::util::{channel_layout::ChannelLayout, color, format::Pixel, format::Sample};
use ffmpeg::{Packet, Rational, codec, frame, media};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;
use parking_lot::{Condvar, Mutex};

use super::audio::{AudioChunk, AudioSource, SOURCE_CHANNELS};
use super::clock::Clock;
use super::details::{AudioDetails, ContainerDetails, VideoDetails};
use super::dovi;
use super::external::{self, SubtitleFile};
use super::hdr;
use super::hwaccel;
use super::orientation::Orientation;
use super::picture::{self, Picture};
use super::source::Source;
use super::subtitles::{Cue, CueQueue, SubtitleDecoder};
use super::tracks::{Track, TrackKind};
#[cfg(target_os = "macos")]
use super::videotoolbox;
#[cfg(not(target_os = "macos"))]
use super::yuv::YuvConverter;

/// Decoded video kept ahead of the playhead, by memory.
const VIDEO_QUEUE_BYTES: usize = 32 * 1024 * 1024;
const VIDEO_QUEUE_MIN_FRAMES: usize = 3;
const VIDEO_QUEUE_MAX_FRAMES: usize = 16;
/// Decoded audio kept ahead of the playhead.
const AUDIO_QUEUE_SECONDS: f64 = 0.75;
/// How far ahead of the playhead a subtitle file is decoded.
const SUBTITLE_FILE_AHEAD: f64 = 2.0;
/// How long the packets of subtitles not playing are kept from before the
/// next packet to decode, so a track picked shows what's on now without a
/// seek, and at most how many of each track.
const SPARE_SUBTITLE_SECONDS: f64 = 10.0;
const SPARE_SUBTITLE_PACKETS: usize = 128;
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
/// How far a skip's target can be from the keyframe before it and still be
/// landed on exactly; further, it lands on the nearest keyframe its way
/// instead of decoding everything in between, in seconds.
const EXACT_SKIP: f64 = 1.0;
/// Packets read after a seek to find the video's first, at most.
const PEEK_PACKETS: usize = 512;
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
    Seek {
        time: f64,
        serial: u64,
        mode: SeekMode,
    },
    /// Play another track (by stream index), or no subtitles. An audio or
    /// video track's packets are read from wherever the demuxer is, so a
    /// seek to the playhead should follow; subtitles start with the packets
    /// kept of them.
    Select {
        kind: TrackKind,
        index: Option<usize>,
    },
    Stop,
}

#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub duration: Option<f64>,
    /// A network stream without an end (it can't be seeked).
    pub live: bool,
    pub has_video: bool,
    pub has_audio: bool,
    /// Display size of the video, after aspect-ratio correction and any cap,
    /// before it's turned to `orientation`.
    pub video_size: Option<(u32, u32)>,
    /// Which way up the video goes, as the file says.
    pub orientation: Orientation,
    pub frame_rate: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    /// The hardware decoding API in use, once frames come out of it.
    pub hardware: Option<&'static str>,
    pub title: Option<String>,
    /// Every track that can be played, in the file's order, then the
    /// subtitle files beside it.
    pub tracks: Vec<Track>,
    /// The tracks playing, by stream index.
    pub video_track: Option<usize>,
    pub audio_track: Option<usize>,
    pub subtitle_track: Option<usize>,
    /// The file's format and the streams playing, as it has them.
    pub container: ContainerDetails,
    pub video_details: Option<VideoDetails>,
    pub audio_details: Option<AudioDetails>,
}

impl MediaInfo {
    /// The video's display size, turned as the file says.
    pub fn upright_size(&self) -> Option<(u32, u32)> {
        let (width, height) = self.video_size?;
        Some(if self.orientation.sideways() {
            (height, width)
        } else {
            (width, height)
        })
    }

    pub fn selected(&self, kind: TrackKind) -> Option<usize> {
        match kind {
            TrackKind::Video => self.video_track,
            TrackKind::Audio => self.audio_track,
            TrackKind::Subtitle => self.subtitle_track,
        }
    }

    pub fn select(&mut self, kind: TrackKind, index: Option<usize>) {
        match kind {
            TrackKind::Video => self.video_track = index,
            TrackKind::Audio => self.audio_track = index,
            TrackKind::Subtitle => self.subtitle_track = index,
        }
    }

    pub fn tracks(&self, kind: TrackKind) -> impl Iterator<Item = &Track> {
        self.tracks.iter().filter(move |track| track.kind == kind)
    }
}

pub struct VideoFrame {
    pub pts: f64,
    /// When it shows in its stream's own time base, to find it again.
    pub timestamp: Option<i64>,
    pub serial: u64,
    pub picture: Picture,
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
    /// Subtitle cues decoded ahead of the playhead.
    pub subtitles: Mutex<CueQueue>,
    /// Set to the current serial once every frame and sample for it has been queued.
    pub eof_serial: AtomicU64,
    /// Seconds of packets read ahead (`f64` bits), infinite once the demuxer
    /// reached the end. Network streams wait for some before playing.
    pub read_ahead: AtomicU64,
    /// Set when the player goes away; aborts network reads that are blocking.
    pub stop: AtomicBool,
    /// Where a skip landed (serial, time) when that isn't where it was
    /// asked to, for the clock.
    pub landed: Mutex<Option<(u64, f64)>>,
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

/// How a seek lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeekMode {
    /// On the frame asked for, however much has to be decoded to get there.
    Exact,
    /// A skip by some seconds, forward or back: on the frame asked for when
    /// a keyframe is near before it, else on the nearest keyframe its way,
    /// which shows at once.
    Skip { forward: bool },
}

/// A seek for the demux thread.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Seek {
    /// In `AV_TIME_BASE` units, on the file's timeline.
    target: i64,
    serial: u64,
    mode: SeekMode,
}

/// Per-seek decoding state handed to the stream decoders.
struct Session<'a> {
    shared: &'a Shared,
    name: &'a str,
    serial: u64,
    /// After a seek, output before this time is decoded but thrown away.
    seek_target: Option<f64>,
    /// The demux thread has said where the last seek landed, so its packets
    /// can be decoded.
    settled: bool,
    /// Timestamp of the start of the media, subtracted from every frame.
    /// Taken from the first packet when the container doesn't say.
    start_offset: Option<f64>,
}

impl Session<'_> {
    fn start_offset(&self) -> f64 {
        self.start_offset.unwrap_or(0.0)
    }
}

/// Open the file or stream; network reads give up once `stop` says so.
pub(super) fn open_input(
    source: &Source,
    stop: impl Fn() -> bool + Send + 'static,
) -> Result<ffmpeg::format::context::Input> {
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
    ffmpeg::format::input_with_interrupt_and_dictionary(source.location(), stop, options)
        .with_context(|| format!("could not open {source}"))
}

/// What the decode thread keeps of each stream once the demux thread has the file.
struct StreamInfo {
    index: usize,
    parameters: codec::Parameters,
    time_base: Rational,
    /// For video, when the file gives a plausible one.
    frame_rate: Option<f64>,
    /// In seconds, when the file says.
    start_time: Option<f64>,
}

impl StreamInfo {
    fn of(stream: &ffmpeg::format::stream::Stream) -> Self {
        let time_base = stream.time_base();
        Self {
            index: stream.index(),
            parameters: stream.parameters().clone(),
            time_base,
            frame_rate: rational(stream.avg_frame_rate())
                .or_else(|| rational(stream.rate()))
                .filter(|fps| (1.0..=480.0).contains(fps)),
            start_time: (stream.start_time() != NO_PTS)
                .then(|| stream.start_time() as f64 * f64::from(time_base)),
        }
    }

    /// The video's size before scaling or aspect-ratio correction.
    fn coded_size(&self) -> Option<(u32, u32)> {
        // SAFETY: the parameters are owned and valid while `self` is.
        let (width, height) = unsafe {
            let raw = &*self.parameters.as_ptr();
            (raw.width, raw.height)
        };
        (width > 0 && height > 0).then_some((width as u32, height as u32))
    }
}

/// The decoders for the tracks playing.
struct Decoders {
    streams: Vec<StreamInfo>,
    options: DecodeOptions,
    /// The video's coded size, which bitmap subtitles are placed on when
    /// they don't say.
    canvas: Option<(u32, u32)>,
    /// The subtitle files beside the video, numbered after its streams.
    files: Vec<PathBuf>,
    video: Option<VideoDecoder>,
    audio: Option<AudioDecoder>,
    subtitle: Option<SubtitleDecoder>,
    /// The packets of the subtitles playing, when they come from a file.
    subtitle_file: Option<SubtitleFile>,
}

impl Decoders {
    fn selected(&self, kind: TrackKind) -> Option<usize> {
        match kind {
            TrackKind::Video => self.video.as_ref().map(|v| v.index),
            TrackKind::Audio => self.audio.as_ref().map(|a| a.index),
            TrackKind::Subtitle => self.subtitle.as_ref().map(|s| s.index),
        }
    }

    fn stream(&self, index: usize) -> Result<&StreamInfo> {
        self.streams
            .get(index)
            .ok_or_else(|| anyhow!("there is no stream {index}"))
    }

    fn open_subtitles(&self, index: usize) -> Result<(SubtitleDecoder, Option<SubtitleFile>)> {
        if let Some(path) = index
            .checked_sub(self.streams.len())
            .and_then(|i| self.files.get(i))
        {
            let (decoder, file) = SubtitleFile::open(path, index, self.canvas)
                .with_context(|| format!("reading {}", path.display()))?;
            return Ok((decoder, Some(file)));
        }
        let stream = self.stream(index)?;
        let decoder = SubtitleDecoder::new(
            index,
            &stream.parameters,
            stream.time_base,
            self.canvas,
            None,
        )?;
        Ok((decoder, None))
    }

    /// Play another track, or no subtitles (video and audio can't be turned
    /// off). A track that can't be decoded leaves the one playing as it is.
    fn switch(
        &mut self,
        kind: TrackKind,
        index: Option<usize>,
        demuxer: &Demuxer,
        shared: &Shared,
        name: &str,
    ) {
        if self.selected(kind) != index
            && let Err(err) = self.try_switch(kind, index, demuxer, shared)
        {
            log::warn!("{name}: can't play that track: {err:#}");
        }
        // Whatever happened, say what plays.
        if let Some(info) = shared.info.lock().as_mut() {
            info.select(kind, self.selected(kind));
        }
    }

    fn try_switch(
        &mut self,
        kind: TrackKind,
        index: Option<usize>,
        demuxer: &Demuxer,
        shared: &Shared,
    ) -> Result<()> {
        let Some(index) = index else {
            if kind == TrackKind::Subtitle {
                demuxer.select(kind, None);
                self.subtitle = None;
                self.subtitle_file = None;
                shared.subtitles.lock().clear();
            }
            return Ok(());
        };
        if kind == TrackKind::Subtitle {
            let (decoder, file) = self.open_subtitles(index)?;
            // The other track's cues go; this one's come from the packets
            // kept of it, or its file, without a seek.
            shared.subtitles.lock().clear();
            // A file's subtitles don't come from the demuxer.
            let queue = match file {
                Some(_) => None,
                None => Some(PacketQueue::new(
                    index,
                    f64::from(self.stream(index)?.time_base),
                )),
            };
            self.subtitle = Some(decoder);
            self.subtitle_file = file;
            demuxer.select(kind, queue);
            return Ok(());
        }
        let stream = self.stream(index)?;
        let queue = Some(PacketQueue::new(index, f64::from(stream.time_base)));
        match kind {
            TrackKind::Video => {
                let options = self.options;
                let decoder = VideoDecoder::new(stream, options.max_short_side, options.hardware)?;
                if let Some(info) = shared.info.lock().as_mut() {
                    info.video_size = Some(decoder.display_size());
                    info.orientation = decoder.orientation;
                    info.frame_rate = Some(1.0 / decoder.frame_duration);
                    info.video_codec = Some(decoder.codec_name());
                    info.video_details = Some(VideoDetails::of(&decoder.decoder));
                    info.hardware = None;
                }
                self.video = Some(decoder);
            }
            TrackKind::Audio => {
                let output = shared
                    .audio
                    .clone()
                    .ok_or_else(|| anyhow!("no audio output device is available"))?;
                let decoder = AudioDecoder::new(stream, output)?;
                if let Some(info) = shared.info.lock().as_mut() {
                    info.audio_codec = Some(decoder.codec_name());
                    info.audio_details = Some(AudioDetails::of(&decoder.decoder));
                }
                self.audio = Some(decoder);
            }
            TrackKind::Subtitle => unreachable!("subtitles are switched above"),
        }
        demuxer.select(kind, queue);
        Ok(())
    }

    /// After a seek to `cue_time`, as the subtitles' own timeline has it
    /// (they may be delayed).
    fn reset(&mut self, cue_time: f64) {
        if let Some(v) = &mut self.video {
            v.reset();
        }
        if let Some(a) = &mut self.audio {
            a.reset();
        }
        if let Some(s) = &mut self.subtitle {
            s.reset();
        }
        if let Some(file) = &mut self.subtitle_file {
            file.seek(cue_time);
        }
    }
}

fn run(
    source: &Source,
    shared: &Arc<Shared>,
    options: DecodeOptions,
    commands: &Receiver<Command>,
) -> Result<()> {
    let interrupt = shared.clone();
    let input = open_input(source, move || interrupt.stop.load(Ordering::Relaxed))?;
    let name = source.name();
    let mut tracks: Vec<Track> = input.streams().filter_map(|s| Track::of(&s)).collect();
    let mut decoders = Decoders {
        streams: input.streams().map(|s| StreamInfo::of(&s)).collect(),
        options,
        canvas: None,
        files: Vec::new(),
        video: None,
        audio: None,
        subtitle: None,
        subtitle_file: None,
    };
    if let Source::File(path) = source {
        for found in external::find(path) {
            let index = decoders.streams.len() + decoders.files.len();
            match found.track(index) {
                Ok(track) => {
                    tracks.push(track);
                    decoders.files.push(found.path);
                }
                Err(err) => log::warn!("{}: skipping it: {err:#}", found.path.display()),
            }
        }
    }

    let mut cover: Option<VideoDecoder> = None;
    if let Some(stream) = input.streams().best(media::Type::Video) {
        let is_cover = stream.disposition().contains(Disposition::ATTACHED_PIC);
        let hardware = options.hardware && !is_cover;
        let info = &decoders.streams[stream.index()];
        match VideoDecoder::new(info, options.max_short_side, hardware) {
            Ok(decoder) if is_cover => cover = Some(decoder),
            Ok(decoder) => {
                decoders.canvas = info.coded_size();
                decoders.video = Some(decoder);
            }
            Err(err) => log::warn!("{source}: skipping video: {err:#}"),
        }
    }

    if let Some(stream) = input.streams().best(media::Type::Audio) {
        match &shared.audio {
            Some(output) => {
                match AudioDecoder::new(&decoders.streams[stream.index()], output.clone()) {
                    Ok(decoder) => decoders.audio = Some(decoder),
                    Err(err) => log::warn!("{source}: skipping audio: {err:#}"),
                }
            }
            None if decoders.video.is_none() => bail!("no audio output device is available"),
            None => log::warn!("{source}: no audio device, playing video only"),
        }
    }
    if decoders.video.is_none() && decoders.audio.is_none() {
        bail!("no playable audio or video streams");
    }

    // Subtitles to show from the start: a subtitle file beside the video,
    // which someone put there to be shown, else the ones the file wants
    // shown, its default track or else a forced one.
    let subtitles = || tracks.iter().filter(|t| t.kind == TrackKind::Subtitle);
    let files = || subtitles().filter(|t| t.file.is_some() && !t.forced);
    let subtitle = files()
        .find(|t| t.default)
        .or_else(|| files().next())
        .or_else(|| subtitles().find(|t| t.default))
        .or_else(|| subtitles().find(|t| t.forced));
    if let Some(track) = subtitle {
        match decoders.open_subtitles(track.index) {
            Ok((decoder, file)) => {
                decoders.subtitle = Some(decoder);
                decoders.subtitle_file = file;
            }
            Err(err) => log::warn!("{source}: skipping subtitles: {err:#}"),
        }
    }

    let start_times: Vec<Option<f64>> = [TrackKind::Video, TrackKind::Audio]
        .into_iter()
        .filter_map(|kind| decoders.selected(kind))
        .map(|index| decoders.streams[index].start_time)
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
    let video = decoders.video.as_ref();
    let audio = decoders.audio.as_ref();
    let info = MediaInfo {
        duration,
        live: source.is_network() && duration.is_none(),
        has_video: video.is_some(),
        has_audio: audio.is_some(),
        video_size: video.map(|v| v.display_size()),
        orientation: video.map(|v| v.orientation).unwrap_or_default(),
        frame_rate: video.map(|v| 1.0 / v.frame_duration),
        video_codec: video.map(|v| v.codec_name()),
        audio_codec: audio.map(|a| a.codec_name()),
        hardware: None,
        title: input.metadata().get("title").map(str::to_owned),
        video_track: decoders.selected(TrackKind::Video),
        audio_track: decoders.selected(TrackKind::Audio),
        subtitle_track: decoders.selected(TrackKind::Subtitle),
        tracks,
        container: ContainerDetails::of(&input),
        video_details: video.map(|v| VideoDetails::of(&v.decoder)),
        audio_details: audio.map(|a| AudioDetails::of(&a.decoder)),
    };
    *shared.info.lock() = Some(info);

    let mut session = Session {
        shared,
        name: &name,
        serial: shared.serial.load(Ordering::Acquire),
        seek_target: None,
        settled: true,
        start_offset,
    };
    reset_queues(shared, session.serial);

    let queue = |kind: TrackKind| {
        decoders
            .selected(kind)
            .map(|index| PacketQueue::new(index, f64::from(decoders.streams[index].time_base)))
    };
    let demuxer = Arc::new(Demuxer {
        packets: Mutex::new(Packets {
            video: queue(TrackKind::Video),
            audio: queue(TrackKind::Audio),
            subtitle: match decoders.subtitle_file {
                Some(_) => None,
                None => queue(TrackKind::Subtitle),
            },
            spare: decoders
                .streams
                .iter()
                .filter(|s| s.parameters.medium() == media::Type::Subtitle)
                .map(|s| PacketQueue::new(s.index, f64::from(s.time_base)))
                .collect(),
            cover_index: cover.as_ref().map(|c| c.index),
            cover: None,
            cover_seen: false,
            bytes: 0,
            serial: session.serial,
            eof: false,
            seek: None,
            landed: None,
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
        let mut selects = Vec::new();
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
                    Command::Seek { time, serial, mode } => seek = Some((time, serial, mode)),
                    Command::Select { kind, index } => selects.push((kind, index)),
                }
                command = match commands.try_recv() {
                    Ok(next) => next,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                };
            }
        }

        for (kind, index) in selects {
            decoders.switch(kind, index, &demuxer, shared, &name);
        }
        if let Some((time, serial, mode)) = seek {
            let target = ((time + session.start_offset()).max(0.0) * AV_TIME_BASE) as i64;
            demuxer.seek(Seek {
                target,
                serial,
                mode,
            });
            let cue_time = shared.subtitles.lock().cue_time(time);
            decoders.reset(cue_time);
            session.serial = serial;
            session.seek_target = Some(time);
            session.settled = false;
            reset_queues(shared, serial);
        }
        // A skip may land on a keyframe away from where it was asked to; its
        // packets wait until that's known.
        if !session.settled
            && let Some(landed) = demuxer.landed(session.serial)
        {
            session.settled = true;
            if let Some(keyframe) = landed {
                let time = keyframe - session.start_offset();
                log::debug!("{name}: skipped to the keyframe at {time:.2}s");
                session.seek_target = Some(time);
                *shared.landed.lock() = Some((session.serial, time));
                if let Some(file) = &mut decoders.subtitle_file {
                    file.seek(shared.subtitles.lock().cue_time(time));
                }
            }
        }

        let mut progressed = false;

        // Decode a packet for each output queue that has room.
        let video_room = session.settled
            && decoders.video.as_ref().is_some_and(|v| {
                !v.finished && shared.video.lock().frames.len() < v.max_queued_frames()
            });
        if video_room && let Some(v) = &mut decoders.video {
            match demuxer.pop(TrackKind::Video, session.serial) {
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
        let audio_room = session.settled
            && decoders
                .audio
                .as_ref()
                .is_some_and(|a| !a.finished && a.has_room());
        if audio_room && let Some(a) = &mut decoders.audio {
            match demuxer.pop(TrackKind::Audio, session.serial) {
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
        // Subtitles are few and small, and each has to be ready before its
        // time, so they are decoded as soon as they are read. A subtitle
        // file is read all at once, so it is decoded as the playhead nears.
        if let Some(s) = &mut decoders.subtitle
            && session.settled
        {
            match &mut decoders.subtitle_file {
                Some(file) => {
                    let now = shared.clock.now();
                    let until = shared.subtitles.lock().cue_time(now) + SUBTITLE_FILE_AHEAD;
                    while let Some(packet) = file.next(until) {
                        progressed = true;
                        // Its times start with the video's.
                        queue_cue(s.decode(packet, 0.0), &session);
                    }
                }
                None => {
                    while let Next::Packet(packet) =
                        demuxer.pop(TrackKind::Subtitle, session.serial)
                    {
                        progressed = true;
                        queue_cue(s.decode(&packet, session.start_offset()), &session);
                    }
                }
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

        let done = decoders.video.as_ref().is_none_or(|v| v.finished)
            && decoders.audio.as_ref().is_none_or(|a| a.finished);
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

/// Add a decoded cue to those around the playhead. Those over before a
/// seek's target are kept too, for going back.
fn queue_cue(cue: Option<Cue>, session: &Session) {
    let Some(cue) = cue else {
        return;
    };
    let now = session.shared.clock.now();
    let mut cues = session.shared.subtitles.lock();
    cues.prune(now);
    cues.push(cue);
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

    /// When the next packet starts, in seconds.
    fn front_time(&self) -> Option<f64> {
        let packet = self.packets.front()?;
        Some(packet.dts().or(packet.pts())? as f64 * self.time_base)
    }

    /// Forget the packets over before `time`, and the oldest past `max`.
    fn keep_from(&mut self, time: f64, max: usize) {
        while let Some(packet) = self.packets.front() {
            let end = packet
                .pts()
                .or(packet.dts())
                .map(|start| (start + packet.duration().max(0)) as f64 * self.time_base);
            if self.packets.len() > max || end.is_some_and(|end| end < time) {
                self.packets.pop_front();
            } else {
                break;
            }
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
    /// Decoded as soon as they are read, so they don't count towards
    /// read-ahead.
    subtitle: Option<PacketQueue>,
    /// The latest packets of each subtitle stream, kept for when it's
    /// picked; the one playing takes its packets from `subtitle` instead.
    spare: Vec<PacketQueue>,
    cover_index: Option<usize>,
    cover: Option<Packet>,
    cover_seen: bool,
    bytes: usize,
    /// The seek generation the queued packets belong to.
    serial: u64,
    eof: bool,
    /// A seek for the demux thread.
    seek: Option<Seek>,
    /// Where the last seek landed, once it has, when that isn't where it
    /// was asked to: the keyframe's time on the file's timeline. Taken by
    /// the decode thread.
    landed: Option<Option<f64>>,
    stopped: bool,
}

impl Packets {
    fn queue(&mut self, kind: TrackKind) -> &mut Option<PacketQueue> {
        match kind {
            TrackKind::Video => &mut self.video,
            TrackKind::Audio => &mut self.audio,
            TrackKind::Subtitle => &mut self.subtitle,
        }
    }

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
        let queue = [&mut self.video, &mut self.audio, &mut self.subtitle]
            .into_iter()
            .flatten()
            .find(|q| q.index == index);
        if let Some(queue) = queue {
            self.bytes += packet.size();
            queue.packets.push_back(packet);
            return;
        }
        if self.cover_index == Some(index) {
            if !self.cover_seen {
                self.cover_seen = true;
                self.cover = Some(packet);
            }
            return;
        }
        // Kept from a little before the next packet to decode, or this one.
        let Some(start) = packet.pts().or(packet.dts()) else {
            return;
        };
        let next = self
            .queues()
            .filter_map(PacketQueue::front_time)
            .reduce(f64::min);
        if let Some(spare) = self.spare.iter_mut().find(|q| q.index == index) {
            let next = next.unwrap_or(start as f64 * spare.time_base);
            spare.packets.push_back(packet);
            spare.keep_from(next - SPARE_SUBTITLE_SECONDS, SPARE_SUBTITLE_PACKETS);
        }
    }

    fn clear(&mut self) {
        for queue in [&mut self.video, &mut self.audio, &mut self.subtitle]
            .into_iter()
            .flatten()
            .chain(&mut self.spare)
        {
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

    fn pop(&self, kind: TrackKind, serial: u64) -> Next {
        let mut packets = self.packets.lock();
        if packets.serial != serial {
            // The demux thread hasn't got to the seek yet.
            return Next::Wait;
        }
        let next = packets
            .queue(kind)
            .as_mut()
            .and_then(|q| q.packets.pop_front());
        match next {
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

    /// Queue packets of another stream for a kind of track, or none.
    /// Subtitles start with the packets kept of them.
    fn select(&self, kind: TrackKind, mut queue: Option<PacketQueue>) {
        let mut packets = self.packets.lock();
        if let Some(queue) = &mut queue
            && let Some(spare) = packets.spare.iter_mut().find(|q| q.index == queue.index)
        {
            queue.packets = std::mem::take(&mut spare.packets);
            packets.bytes += queue.packets.iter().map(Packet::size).sum::<usize>();
        }
        if let Some(old) = std::mem::replace(packets.queue(kind), queue) {
            packets.bytes -= old.packets.iter().map(Packet::size).sum::<usize>();
        }
        self.publish(&packets);
        self.wake.notify_one();
    }

    fn take_cover(&self) -> Option<Packet> {
        self.packets.lock().cover.take()
    }

    fn seek(&self, seek: Seek) {
        let mut packets = self.packets.lock();
        packets.clear();
        packets.seek = Some(seek);
        packets.landed = None;
        self.publish(&packets);
        self.wake.notify_one();
    }

    /// Once the demux thread has carried out the seek for `serial`: the
    /// keyframe it landed on, if not where it was asked to.
    fn landed(&self, serial: u64) -> Option<Option<f64>> {
        let mut packets = self.packets.lock();
        if packets.serial != serial {
            return None;
        }
        Some(packets.landed.take().flatten())
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

        if let Some(seek) = seek {
            let video = {
                let packets = demuxer.packets.lock();
                packets.video.as_ref().map(|q| (q.index, q.time_base))
            };
            let landing = land(&mut input, seek, video, name);
            let mut packets = demuxer.packets.lock();
            // A newer seek may have come in meanwhile; it runs next time round.
            if packets.seek == Some(seek) {
                packets.seek = None;
                packets.serial = seek.serial;
                packets.landed = Some(landing.keyframe);
                for packet in landing.packets {
                    packets.push(packet);
                }
                match landing.error {
                    None => {}
                    Some(_) if stopping() => return,
                    Some(ffmpeg::Error::Eof) => packets.eof = true,
                    Some(err) => {
                        log::warn!("{name}: read error, treating it as the end: {err}");
                        packets.eof = true;
                    }
                }
                demuxer.publish(&packets);
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

/// What carrying out a seek read.
#[derive(Default)]
struct Landing {
    /// Read to see where it landed, in the file's order.
    packets: Vec<Packet>,
    /// The keyframe landed on, on the file's timeline, when that isn't
    /// where the seek was asked to.
    keyframe: Option<f64>,
    /// What stopped the reading short.
    error: Option<ffmpeg::Error>,
}

/// Seek `input` to the keyframe before the target. A skip whose target is
/// far past it lands on that keyframe instead when going back, or on the
/// one after the target when going forward, so it shows at once.
fn land(
    input: &mut ffmpeg::format::context::Input,
    seek: Seek,
    video: Option<(usize, f64)>,
    name: &str,
) -> Landing {
    let target = seek.target;
    let seconds = target as f64 / AV_TIME_BASE;
    if let Err(err) = input.seek(target, ..target) {
        log::warn!("{name}: seek to {seconds:.2}s failed: {err}");
    }
    let (SeekMode::Skip { forward }, Some(video)) = (seek.mode, video) else {
        return Landing::default();
    };
    let mut landing = read_to_video(input, video);
    let Some(keyframe) = landing.keyframe.filter(|k| seconds - k > EXACT_SKIP) else {
        landing.keyframe = None;
        return landing;
    };
    if !forward {
        landing.keyframe = Some(keyframe);
        return landing;
    }
    if input.seek(target, target..).is_ok() {
        let mut after = read_to_video(input, video);
        if after.keyframe.is_some_and(|k| k >= seconds) {
            return after;
        }
        // Where it went isn't past the target after all: decode from there.
        after.keyframe = None;
        return after;
    }
    // No keyframe after it: back to the one before, to decode up to it.
    if let Err(err) = input.seek(target, ..target) {
        log::warn!("{name}: seek to {seconds:.2}s failed: {err}");
    }
    Landing::default()
}

/// Read up to the first packet of the `video` stream (index, time base),
/// keeping its time in `keyframe`.
fn read_to_video(input: &mut ffmpeg::format::context::Input, video: (usize, f64)) -> Landing {
    let mut landing = Landing::default();
    while landing.packets.len() < PEEK_PACKETS {
        let mut packet = Packet::empty();
        match packet.read(input) {
            Ok(()) => {
                let found = packet.stream() == video.0;
                if found {
                    landing.keyframe = packet.pts().or(packet.dts()).map(|t| t as f64 * video.1);
                }
                landing.packets.push(packet);
                if found {
                    break;
                }
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                thread::sleep(IDLE_WAIT);
            }
            Err(err) => {
                landing.error = Some(err);
                break;
            }
        }
    }
    landing
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

/// What a frame is converted to before it becomes BGRA.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Light {
    /// Straight to BGRA, by swscale.
    Sdr,
    /// 16-bit RGB, then tone mapped.
    Hdr(hdr::Transfer),
    /// 16-bit 4:4:4 planes, reshaped, then tone mapped.
    DolbyVision,
}

pub(super) struct Scaler {
    context: scaling::Context,
    key: (Pixel, u32, u32, Light),
    width: u32,
    height: u32,
    output: frame::Video,
    /// For HDR frames, Dolby Vision's included (always PQ).
    tone_mapper: Option<hdr::ToneMapper>,
}

struct VideoDecoder {
    index: usize,
    decoder: ffmpeg::decoder::Video,
    /// Kept to reopen the decoder in software if the hardware one fails.
    parameters: codec::Parameters,
    /// Which way up the pictures go, from the stream's display matrix.
    orientation: Orientation,
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
    /// Turns hardware frames into surfaces, created on first use; `Err` once
    /// that failed, after which frames are copied to memory instead.
    #[cfg(target_os = "macos")]
    surfaces: Option<Result<videotoolbox::Converter, ()>>,
    /// Hands frames to gpui as their planes, for the GPU to convert.
    #[cfg(not(target_os = "macos"))]
    planes: YuvConverter,
    errors: u32,
    next_pts: f64,
    finished: bool,
}

/// Open a decoder for the stream, on the GPU when asked and possible.
pub(super) fn open_video(
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
    fn new(stream: &StreamInfo, max_short_side: Option<u32>, hardware: bool) -> Result<Self> {
        let parameters = stream.parameters.clone();
        let (decoder, hardware) = open_video(&parameters, hardware)?;
        if decoder.width() == 0 || decoder.height() == 0 {
            bail!("video stream has no dimensions");
        }
        let frame_duration = stream.frame_rate.map_or(1.0 / 25.0, |fps| 1.0 / fps);
        Ok(Self {
            index: stream.index,
            decoder,
            orientation: Orientation::of(&parameters),
            parameters,
            time_base: f64::from(stream.time_base),
            frame_duration,
            max_short_side,
            scaler: None,
            decoded: frame::Video::empty(),
            downloaded: frame::Video::empty(),
            hardware,
            reported: None,
            #[cfg(target_os = "macos")]
            surfaces: None,
            #[cfg(not(target_os = "macos"))]
            planes: YuvConverter::default(),
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
        // Frames other frames don't refer to aren't decoded on the way to a
        // seek's target, as they wouldn't be shown: it gets there sooner.
        let before_target = session.seek_target.is_some_and(|target| {
            packet_time(packet, self.time_base).is_some_and(|time| {
                time - session.start_offset() + 2.0 * self.frame_duration <= target
            })
        });
        self.decoder.skip_frame(if before_target {
            Discard::NonReference
        } else {
            Discard::Default
        });
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

            let timestamp = self.decoded.timestamp();
            let pts = timestamp
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
            if let Some(picture) = self.picture(on_gpu, session) {
                session.shared.video.lock().frames.push_back(VideoFrame {
                    pts,
                    timestamp,
                    serial: session.serial,
                    picture,
                });
            }
        }
    }

    /// Make the decoded frame drawable.
    fn picture(&mut self, on_gpu: bool, session: &Session) -> Option<Picture> {
        #[cfg(target_os = "macos")]
        if on_gpu && let Some(surface) = self.surface(session) {
            return Some(Picture::Surface(surface));
        }
        // Elsewhere, frames decoded on the GPU are copied back to draw.
        #[cfg(not(target_os = "macos"))]
        let _ = session;
        let frame = if on_gpu {
            if let Err(err) = hwaccel::download(&self.decoded, &mut self.downloaded) {
                log::warn!("{err:#}");
                return None;
            }
            &self.downloaded
        } else {
            &self.decoded
        };
        #[cfg(not(target_os = "macos"))]
        if gpu_conversion() {
            match self.planes.convert(frame, self.max_short_side) {
                Some(Ok(planes)) => return Some(Picture::Yuv(planes)),
                Some(Err(err)) => log::warn!("handing the frame to the GPU failed: {err:#}"),
                None => {}
            }
        }
        convert(&mut self.scaler, self.max_short_side, frame)
            .inspect_err(|err| log::warn!("video conversion failed: {err:#}"))
            .ok()
            .map(Picture::Image)
    }

    /// Convert a hardware frame without it leaving the GPU. `None` if that
    /// isn't possible, in which case it never is again for this video.
    #[cfg(target_os = "macos")]
    fn surface(&mut self, session: &Session) -> Option<videotoolbox::Surface> {
        let (width, height) = output_size(
            self.decoded.width(),
            self.decoded.height(),
            self.decoded.aspect_ratio(),
            self.max_short_side,
        );
        let converter = self
            .surfaces
            .get_or_insert_with(|| {
                videotoolbox::Converter::new().map_err(|err| {
                    log::warn!("{}: {err:#}; copying frames from the GPU", session.name)
                })
            })
            .as_mut()
            .ok()?;
        match converter.convert(&self.decoded, width, height) {
            Ok(surface) => Some(surface),
            Err(err) => {
                log::warn!("{}: {err:#}; copying frames from the GPU", session.name);
                self.surfaces = Some(Err(()));
                None
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

/// Whether frames go to gpui as their planes, for the GPU to convert:
/// unless `PLAYS_GPU_CONVERSION=0`, which converts them on the CPU, as
/// before, to compare the two.
#[cfg(not(target_os = "macos"))]
fn gpu_conversion() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PLAYS_GPU_CONVERSION").is_none_or(|v| v != "0"))
}

/// Convert a decoded frame to a BGRA image gpui can draw.
fn convert(
    scaler: &mut Option<Scaler>,
    max_short_side: Option<u32>,
    frame: &frame::Video,
) -> Result<Arc<RenderImage>> {
    let (width, height, pixels) = convert_pixels(scaler, max_short_side, frame)?;
    // gpui reads image bytes as BGRA, which is exactly what we converted to.
    let buffer = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| anyhow!("frame buffer has the wrong size"))?;
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// Convert a decoded frame to tightly packed BGRA for an SDR screen, its
/// width and height and its pixels: corrected for non-square pixels,
/// scaled down to `max_short_side`, HDR tone mapped, with the gamma set.
pub(super) fn convert_pixels(
    scaler: &mut Option<Scaler>,
    max_short_side: Option<u32>,
    frame: &frame::Video,
) -> Result<(u32, u32, Vec<u8>)> {
    let reshaping = dovi::Reshaping::of(frame);
    let light = match (&reshaping, hdr::Transfer::of(frame)) {
        (Some(_), _) => Light::DolbyVision,
        (None, Some(transfer)) => Light::Hdr(transfer),
        (None, None) => Light::Sdr,
    };
    let key = (frame.format(), frame.width(), frame.height(), light);
    if scaler.as_ref().is_none_or(|s| s.key != key) {
        let (width, height) = output_size(
            frame.width(),
            frame.height(),
            frame.aspect_ratio(),
            max_short_side,
        );
        let (format, tone_mapper) = match light {
            Light::Sdr => (Pixel::BGRA, None),
            Light::Hdr(transfer) => (Pixel::RGB48LE, Some(hdr::ToneMapper::new(transfer))),
            Light::DolbyVision => (
                Pixel::YUV444P16LE,
                Some(hdr::ToneMapper::new(hdr::Transfer::Pq)),
            ),
        };
        let mut context = scaling::Context::get(
            key.0,
            key.1,
            key.2,
            format,
            width,
            height,
            scaling::Flags::BILINEAR,
        )
        .context("creating the pixel converter")?;
        // Dolby Vision's samples aren't YCbCr: they are only scaled, never
        // converted.
        if light != Light::DolbyVision {
            set_colorspace(&mut context, frame);
        }
        *scaler = Some(Scaler {
            context,
            key,
            width,
            height,
            output: frame::Video::empty(),
            tone_mapper,
        });
    }
    let scaler = scaler.as_mut().expect("scaler was just created");
    scaler.context.run(frame, &mut scaler.output)?;
    let gamma = picture::gamma(light != Light::Sdr);
    if let Some(mapper) = &mut scaler.tone_mapper {
        mapper.set_gamma(gamma);
    }

    let (width, height) = (scaler.width, scaler.height);
    let output = &scaler.output;
    let pixels = match (&scaler.tone_mapper, &reshaping) {
        (Some(mapper), Some(reshaping)) => reshaping.render(
            mapper,
            [0, 1, 2].map(|plane| (output.data(plane), output.stride(plane))),
            width as usize,
            height as usize,
        ),
        (Some(mapper), None) => mapper.run(
            output.data(0),
            output.stride(0),
            width as usize,
            height as usize,
        ),
        (None, _) if gamma == 1.0 => {
            let (stride, data) = (output.stride(0), output.data(0));
            let row = width as usize * 4;
            let mut pixels = Vec::with_capacity(row * height as usize);
            for y in 0..height as usize {
                pixels.extend_from_slice(&data[y * stride..y * stride + row]);
            }
            pixels
        }
        (None, _) => {
            let (stride, data) = (output.stride(0), output.data(0));
            let table = picture::gamma_table(gamma);
            picture::bgra_rows(width as usize, height as usize, |y, out| {
                let (row, _) = data[y * stride..][..out.len()].as_chunks::<4>();
                let (out, _) = out.as_chunks_mut::<4>();
                for (out, &[b, g, r, a]) in out.iter_mut().zip(row) {
                    *out = [table[b as usize], table[g as usize], table[r as usize], a];
                }
            })
        }
    };
    Ok((width, height, pixels))
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
pub(super) fn set_colorspace(context: &mut scaling::Context, frame: &frame::Video) {
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
    fn new(stream: &StreamInfo, source: Arc<AudioSource>) -> Result<Self> {
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

    fn codec_name(&self) -> String {
        codec_name(self.decoder.id())
    }

    fn output_rate(&self) -> u32 {
        self.source.sample_rate()
    }

    fn has_room(&self) -> bool {
        self.source.queue.lock().buffered_seconds() < AUDIO_QUEUE_SECONDS
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
            self.source.queue.lock().push(AudioChunk {
                pts,
                samples,
                pos: 0,
                rate: out_rate,
            });
        }
        Ok(())
    }
}
