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
//! queued. Cover art in a file's tags shows for sound; for sound with none,
//! a cover file beside it does (`cover_file`).
//! It only decodes while an output queue has room, so memory stays bounded
//! however much is read ahead. One video, one audio and one subtitle track
//! play at a time; another can be picked while playing.

mod audio;
mod convert;
mod cover_file;
mod demux;
pub(super) mod fetch;
mod hls;
mod shared;
mod video;

use std::any::Any;
use std::ffi::CStr;
use std::fmt::Display;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use ffmpeg::format::stream::Disposition;
use ffmpeg::{Rational, codec, media};
use ffmpeg_next as ffmpeg;
use parking_lot::{Condvar, Mutex};

pub(crate) use convert::output_size;
pub(super) use convert::{convert_pixels, set_colorspace};
#[cfg(test)]
pub use demux::PANICS_IN_DEMUX;
pub use shared::{Shared, VideoFrame};
pub(super) use video::open_video;

use super::chapters::Chapter;
use super::details::{AudioDetails, ContainerDetails, VideoDetails};
use super::external::{self, Reading, SubtitleFile};
use super::orientation::Orientation;
use super::picking::Wanted;
use super::source::Source;
use super::subtitles::{Cue, SubtitleDecoder};
use super::tracks::{Track, TrackKind};
use audio::AudioDecoder;
use demux::{
    Demuxer, FILE_READ_AHEAD, Next, PacketQueue, Packets, STREAM_READ_AHEAD, StopOnDrop, demux,
    packet_time,
};
use video::VideoDecoder;

/// How far ahead of the playhead a subtitle file is decoded.
const SUBTITLE_FILE_AHEAD: f64 = 2.0;
/// Network reads and connections give up after this long without data.
const NETWORK_TIMEOUT_MICROS: &str = "15000000";
/// How long the decode thread waits, at most, with nothing to do. What it
/// waits for rings it awake sooner; only time brings a subtitle file's
/// next cues within reach, and they are decoded seconds early.
const IDLE_WAIT: Duration = Duration::from_millis(250);
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
        /// Its number, as `Shared::select_track` gave it.
        pick: u64,
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
    /// Who made it, as its tags say; for a song, the album it's on, and
    /// whose album that is.
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
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
    /// The file's parts, in order, as its chapters say.
    pub chapters: Vec<Chapter>,
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
    wanted: Wanted,
    commands: Receiver<Command>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name(format!("decode {}", source.name()))
        .spawn(move || {
            guarded(&shared, &source, || {
                run(&source, &shared, options, &wanted, &commands)
            });
        })
        .expect("spawning a decoder thread")
}

/// Run a decoder thread's `body`, saying why it failed, a panic as much
/// as an error, so the block says it can't play rather than waiting for
/// ever. The first thread to fail says why.
fn guarded(shared: &Shared, source: &impl Display, body: impl FnOnce() -> Result<()>) {
    let failed = match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => return,
        Ok(Err(err)) => format!("{err:#}"),
        Err(panic) => format!("the decoder stopped: {}", panic_message(&*panic)),
    };
    if shared.stopped() {
        // Closed meanwhile; nobody is looking.
        return;
    }
    log::error!("{source}: {failed}");
    shared.fail(failed);
}

/// What a caught `panic` said.
pub fn panic_message(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("for no reason it gave")
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
    /// The slider being dragged: on the frame asked for, showing frames on
    /// the way as they are decoded, and nothing after it, with no sound.
    /// One a little ahead of the frame decoded last decodes on from there.
    Scrub,
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
    /// The last seek is a scrub (`SeekMode::Scrub`).
    scrub: bool,
    /// Timestamp of the start of the media, subtracted from every frame.
    /// Taken from the first packet when the container doesn't say.
    start_offset: Option<f64>,
}

impl Session<'_> {
    fn start_offset(&self) -> f64 {
        self.start_offset.unwrap_or(0.0)
    }
}

/// Open the file or stream; network reads give up once `stop` says so. An
/// HLS master playlist is cut down to the variant that plays within
/// `max_short_side` (`hls`), or played whole if that won't open.
pub(super) fn open_input(
    source: &Source,
    max_short_side: Option<u32>,
    stop: impl Fn() -> bool + Clone + Send + 'static,
) -> Result<ffmpeg::format::context::Input> {
    if let Some(variant) = hls::variant(source, max_short_side, stop.clone())? {
        match open(Path::new(&variant), source, stop.clone()) {
            Ok(input) => return Ok(input),
            Err(err) if stop() => return Err(err),
            Err(err) => {
                log::warn!("{source}: playing every variant, as one alone won't open: {err:#}")
            }
        }
    }
    open(source.location(), source, stop)
}

/// Open `location`, where `source` is.
fn open(
    location: &Path,
    source: &Source,
    stop: impl Fn() -> bool + Send + 'static,
) -> Result<ffmpeg::format::context::Input> {
    let options = if source.is_network() {
        network_options()
    } else {
        ffmpeg::Dictionary::new()
    };
    ffmpeg::format::input_with_interrupt_and_dictionary(location, stop, options)
        .with_context(|| format!("could not open {source}"))
}

/// What a stream is opened with.
fn network_options() -> ffmpeg::Dictionary<'static> {
    let mut options = ffmpeg::Dictionary::new();
    options.set("rw_timeout", NETWORK_TIMEOUT_MICROS);
    options.set("timeout", NETWORK_TIMEOUT_MICROS);
    options.set("reconnect", "1");
    options.set("reconnect_streamed", "1");
    options.set("reconnect_delay_max", "5");
    // Like most players: UDP rarely makes it through NAT and firewalls.
    options.set("rtsp_transport", "tcp");
    options
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
        // SAFETY: the parameters are owned and valid while `self` is. Read
        // through the pointer, never a reference: a file can hold numbers
        // their Rust enums have no variant for.
        let (width, height) = unsafe {
            let raw = self.parameters.as_ptr();
            ((*raw).width, (*raw).height)
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
    /// The subtitle file to play next, being read; what plays plays on
    /// until it is.
    reading: Option<Reading>,
    /// The number of the last pick of subtitles done.
    subtitle_pick: u64,
}

impl Decoders {
    /// The track playing, or for subtitles the file being read to play.
    fn selected(&self, kind: TrackKind) -> Option<usize> {
        match kind {
            TrackKind::Video => self.video.as_ref().map(|v| v.index),
            TrackKind::Audio => self.audio.as_ref().map(|a| a.index),
            TrackKind::Subtitle => self
                .reading
                .as_ref()
                .map(|reading| reading.index)
                .or_else(|| self.subtitle.as_ref().map(|s| s.index)),
        }
    }

    fn stream(&self, index: usize) -> Result<&StreamInfo> {
        self.streams
            .get(index)
            .ok_or_else(|| anyhow!("there is no stream {index}"))
    }

    /// A decoder for subtitle stream `index`.
    fn open_subtitles(&self, index: usize) -> Result<SubtitleDecoder> {
        let stream = self.stream(index)?;
        SubtitleDecoder::new(
            index,
            &stream.parameters,
            stream.time_base,
            self.canvas,
            None,
        )
    }

    /// Start reading subtitles `index` if they come from a file, on a
    /// thread of its own, as a file can be large and far away: they play
    /// once it is read (`take_read`). Says whether they do.
    fn read_subtitles(&mut self, index: usize, shared: &Arc<Shared>) -> Result<bool> {
        let Some(path) = index
            .checked_sub(self.streams.len())
            .and_then(|i| self.files.get(i))
        else {
            return Ok(false);
        };
        let shared = shared.clone();
        let reading = Reading::start(path.clone(), index, self.canvas, move || {
            shared.ring();
        })?;
        self.reading = Some(reading);
        Ok(true)
    }

    /// Play the subtitle file being read once it is, from a little before
    /// the playhead, in place of what played. Says whether it was read.
    fn take_read(&mut self, demuxer: &Demuxer, shared: &Shared, name: &str) -> bool {
        let Some(read) = self.reading.as_ref().and_then(Reading::take) else {
            return false;
        };
        self.reading = None;
        match read {
            Ok((decoder, mut file)) => {
                let mut cues = shared.subtitles();
                file.seek(cues.cue_time(shared.clock().now()));
                // The other track's cues go.
                cues.clear();
                drop(cues);
                self.subtitle = Some(decoder);
                self.subtitle_file = Some(file);
                // A file's subtitles don't come from the demuxer.
                demuxer.select(TrackKind::Subtitle, None);
            }
            Err(err) => log::warn!("{name}: can't play that track: {err:#}"),
        }
        shared.played(
            TrackKind::Subtitle,
            self.selected(TrackKind::Subtitle),
            self.subtitle_pick,
        );
        true
    }

    /// Play another track, or no subtitles (video and audio can't be turned
    /// off), as pick number `pick` asks. A track that can't be decoded
    /// leaves the one playing as it is.
    fn switch(
        &mut self,
        kind: TrackKind,
        index: Option<usize>,
        pick: u64,
        demuxer: &Demuxer,
        shared: &Arc<Shared>,
        name: &str,
    ) {
        if self.selected(kind) != index
            && let Err(err) = self.try_switch(kind, index, demuxer, shared)
        {
            log::warn!("{name}: can't play that track: {err:#}");
        }
        if kind == TrackKind::Subtitle {
            self.subtitle_pick = pick;
        }
        // Whatever happened, say what plays.
        shared.played(kind, self.selected(kind), pick);
    }

    fn try_switch(
        &mut self,
        kind: TrackKind,
        index: Option<usize>,
        demuxer: &Demuxer,
        shared: &Arc<Shared>,
    ) -> Result<()> {
        let Some(index) = index else {
            if kind == TrackKind::Subtitle {
                demuxer.select(kind, None);
                self.subtitle = None;
                self.subtitle_file = None;
                self.reading = None;
                shared.subtitles().clear();
            }
            return Ok(());
        };
        if kind == TrackKind::Subtitle {
            // Back to what plays, while another was read.
            if self.subtitle.as_ref().is_some_and(|s| s.index == index) {
                self.reading = None;
                return Ok(());
            }
            if self.read_subtitles(index, shared)? {
                return Ok(());
            }
            let decoder = self.open_subtitles(index)?;
            self.reading = None;
            // The other track's cues go; this one's come from the packets
            // kept of it, without a seek.
            shared.subtitles().clear();
            let queue = PacketQueue::new(index, f64::from(self.stream(index)?.time_base));
            self.subtitle = Some(decoder);
            self.subtitle_file = None;
            demuxer.select(kind, Some(queue));
            return Ok(());
        }
        let stream = self.stream(index)?;
        let queue = Some(PacketQueue::new(index, f64::from(stream.time_base)));
        match kind {
            TrackKind::Video => {
                let options = self.options;
                let decoder = VideoDecoder::new(stream, options.max_short_side, options.hardware)?;
                shared.update_info(|info| {
                    info.video_size = Some(decoder.display_size());
                    info.orientation = decoder.orientation;
                    info.frame_rate = Some(1.0 / decoder.frame_duration);
                    info.video_codec = Some(decoder.codec_name());
                    info.video_details = Some(VideoDetails::of(&decoder.decoder));
                    info.hardware = None;
                });
                self.video = Some(decoder);
            }
            TrackKind::Audio => {
                let output = shared
                    .audio()
                    .cloned()
                    .ok_or_else(|| anyhow!("no audio output device is available"))?;
                let decoder = AudioDecoder::new(stream, output)?;
                shared.update_info(|info| {
                    info.audio_codec = Some(decoder.codec_name());
                    info.audio_details = Some(AudioDetails::of(&decoder.decoder));
                });
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
    wanted: &Wanted,
    commands: &Receiver<Command>,
) -> Result<()> {
    // The subtitle files beside it are looked for, and opened, while it
    // opens: each can take a while.
    let looking = look_for_subtitle_files(source);
    let interrupt = shared.clone();
    let input = open_input(source, options.max_short_side, move || interrupt.stopped())?;
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
        reading: None,
        subtitle_pick: 0,
    };
    if let Some(looking) = looking {
        add_subtitle_files(looking, &mut tracks, &mut decoders);
    }

    let mut cover = open_best_video(&input, &mut decoders, options, source);
    let audio = wanted.audio_track(&tracks);
    open_audio(&input, audio, &mut decoders, shared, source)?;
    if decoders.video.is_none() && decoders.audio.is_none() {
        bail!("no playable audio or video streams");
    }
    // Sound with no picture of its own shows the one kept beside it.
    if cover.is_none() && decoders.video.is_none() {
        cover_file::show(source, shared, options.max_short_side);
    }
    start_subtitles(
        wanted.subtitle_track(&tracks),
        &mut decoders,
        shared,
        source,
    );

    let start_offset = media_start(&decoders);
    shared.set_info(media_info(source, &input, &decoders, tracks, start_offset));
    if let Some(audio) = &mut decoders.audio {
        audio.due_from_the_start = start_offset.is_some();
    }

    let mut session = Session {
        shared,
        name: &name,
        serial: shared.serial(),
        seek_target: None,
        settled: true,
        scrub: false,
        start_offset,
    };
    shared.reset_queues(session.serial);

    let demuxer = start_demuxer(
        input,
        source,
        &decoders,
        cover.as_ref(),
        session.serial,
        shared,
        &name,
    )?;
    // Stops the demux thread however this function returns.
    let _demuxer_guard = StopOnDrop(demuxer.clone());

    let mut wait: Option<Duration> = None;
    loop {
        if let Some(timeout) = wait {
            shared.wait(timeout);
        }
        let Some(Received { seek, selects }) = receive(commands) else {
            return Ok(());
        };
        for (kind, index, pick) in selects {
            decoders.switch(kind, index, pick, &demuxer, shared, &name);
        }
        if let Some((time, serial, mode)) = seek {
            seek_to(&mut session, &mut decoders, &demuxer, time, serial, mode);
        }
        settle(&mut session, &mut decoders, &demuxer);

        // Decode a packet for each output queue that has room.
        let mut progressed = decode_video(&mut session, &mut decoders, &demuxer);
        progressed |= decode_audio(&mut session, &mut decoders, &demuxer);
        progressed |= decode_subtitles(&session, &mut decoders, &demuxer);
        if let Some(c) = &mut cover
            && let Some(packet) = demuxer.take_cover()
            && let Some(image) = c.decode_still(&packet)
        {
            shared.set_cover(image);
        }

        let done = decoders.video.as_ref().is_none_or(|v| v.finished)
            && decoders.audio.as_ref().is_none_or(|a| a.finished);
        if done {
            shared.set_eof(session.serial);
        }

        // Sound playing makes room as it goes, with nothing to ring.
        let sound_room = decoders
            .audio
            .as_ref()
            .filter(|a| !done && !a.finished)
            .and_then(AudioDecoder::until_room);
        wait = (!progressed).then(|| sound_room.map_or(IDLE_WAIT, |room| room.min(IDLE_WAIT)));
    }
}

/// Look for the subtitle files beside a file, on a thread of their own.
fn look_for_subtitle_files(source: &Source) -> Option<JoinHandle<Vec<Track>>> {
    match source {
        Source::File(path) => {
            let path = path.clone();
            thread::Builder::new()
                .name("subtitle files".into())
                .spawn(move || external::tracks(&path))
                .inspect_err(|err| log::warn!("{source}: not looking for subtitle files: {err}"))
                .ok()
        }
        _ => None,
    }
}

/// Once they're found, add the subtitle files beside it to its tracks,
/// after its own streams.
fn add_subtitle_files(
    looking: JoinHandle<Vec<Track>>,
    tracks: &mut Vec<Track>,
    decoders: &mut Decoders,
) {
    for mut track in looking.join().unwrap_or_default() {
        let Some(path) = track.file.clone() else {
            continue;
        };
        track.index = decoders.streams.len() + decoders.files.len();
        tracks.push(track);
        decoders.files.push(path);
    }
}

/// Play the best video stream, or keep it as the cover when it is a
/// picture attached to the file, which is returned.
fn open_best_video(
    input: &ffmpeg::format::context::Input,
    decoders: &mut Decoders,
    options: DecodeOptions,
    source: &Source,
) -> Option<VideoDecoder> {
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
    cover
}

/// Play the audio stream `wanted` (`picking::Wanted`), else the best one.
/// With no audio device a video plays without it, and sound alone can't
/// play.
fn open_audio(
    input: &ffmpeg::format::context::Input,
    wanted: Option<usize>,
    decoders: &mut Decoders,
    shared: &Shared,
    source: &Source,
) -> Result<()> {
    let best = || input.streams().best(media::Type::Audio).map(|s| s.index());
    if let Some(index) = wanted.or_else(best) {
        match shared.audio() {
            Some(output) => match AudioDecoder::new(&decoders.streams[index], output.clone()) {
                Ok(decoder) => decoders.audio = Some(decoder),
                Err(err) => log::warn!("{source}: skipping audio: {err:#}"),
            },
            None if decoders.video.is_none() => bail!("no audio output device is available"),
            None => log::warn!("{source}: no audio device, playing video only"),
        }
    }
    Ok(())
}

/// Start the subtitles to show from the start, track `index`, if any, as
/// `picking::Wanted` picks them.
fn start_subtitles(
    index: Option<usize>,
    decoders: &mut Decoders,
    shared: &Arc<Shared>,
    source: &Source,
) {
    if let Some(index) = index {
        let started = decoders.read_subtitles(index, shared).and_then(|read| {
            if !read {
                decoders.subtitle = Some(decoders.open_subtitles(index)?);
            }
            Ok(())
        });
        if let Err(err) = started {
            log::warn!("{source}: skipping subtitles: {err:#}");
        }
    }
}

/// Where the media starts: the earliest start of the video and audio
/// playing, when each of them says.
fn media_start(decoders: &Decoders) -> Option<f64> {
    let start_times: Vec<Option<f64>> = [TrackKind::Video, TrackKind::Audio]
        .into_iter()
        .filter_map(|kind| decoders.selected(kind))
        .map(|index| decoders.streams[index].start_time)
        .collect();
    if start_times.iter().all(Option::is_some) {
        Some(
            start_times
                .into_iter()
                .flatten()
                .fold(f64::INFINITY, f64::min),
        )
    } else {
        None
    }
}

/// What the player shows of the media opened, before anything plays.
fn media_info(
    source: &Source,
    input: &ffmpeg::format::context::Input,
    decoders: &Decoders,
    tracks: Vec<Track>,
    start_offset: Option<f64>,
) -> MediaInfo {
    let duration = (input.duration() > 0).then(|| input.duration() as f64 / AV_TIME_BASE);
    // On the playback timeline, which starts where the streams do.
    let offset = start_offset.unwrap_or(0.0);
    let chapters = chapters(input, offset);
    let video = decoders.video.as_ref();
    let audio = decoders.audio.as_ref();
    // A file with only sound may keep its tags on it, as Ogg does.
    let sound = video
        .is_none()
        .then(|| decoders.selected(TrackKind::Audio))
        .flatten();
    let tag = |key| tag(input, sound, key);
    MediaInfo {
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
        title: tag(c"title"),
        artist: tag(c"artist"),
        album: tag(c"album"),
        album_artist: tag(c"album_artist"),
        video_track: decoders.selected(TrackKind::Video),
        audio_track: decoders.selected(TrackKind::Audio),
        subtitle_track: decoders.selected(TrackKind::Subtitle),
        tracks,
        container: ContainerDetails::of(input),
        video_details: video.map(|v| VideoDetails::of(&v.decoder)),
        audio_details: audio.map(|a| AudioDetails::of(&a.decoder)),
        chapters,
    }
}

/// The file's tag `key`, or the stream `sound`'s if the file has none.
fn tag(input: &ffmpeg::format::context::Input, sound: Option<usize>, key: &CStr) -> Option<String> {
    ffmpeg_safe::tag(&input.metadata(), key).or_else(|| {
        sound
            .and_then(|index| input.stream(index))
            .and_then(|stream| ffmpeg_safe::tag(&stream.metadata(), key))
    })
}

/// The file's chapters, in order, on the playback timeline, which starts
/// `offset` into the file.
fn chapters(input: &ffmpeg::format::context::Input, offset: f64) -> Vec<Chapter> {
    let mut chapters: Vec<Chapter> = input
        .chapters()
        .map(|chapter| {
            let base = chapter.time_base();
            let seconds =
                |at: i64| at as f64 * f64::from(base.numerator()) / f64::from(base.denominator());
            Chapter {
                start: (seconds(chapter.start()) - offset).max(0.0),
                end: seconds(chapter.end()) - offset,
                title: ffmpeg_safe::tag(&chapter.metadata(), c"title"),
            }
        })
        .collect();
    chapters.sort_by(|a, b| a.start.total_cmp(&b.start));
    chapters
}

/// Start the demux thread reading the tracks the decoders play, and the
/// other subtitle tracks' packets to keep.
fn start_demuxer(
    input: ffmpeg::format::context::Input,
    source: &Source,
    decoders: &Decoders,
    cover: Option<&VideoDecoder>,
    serial: u64,
    shared: &Arc<Shared>,
    name: &str,
) -> Result<Arc<Demuxer>> {
    let queue = |kind: TrackKind| {
        decoders
            .selected(kind)
            .map(|index| PacketQueue::new(index, f64::from(decoders.streams[index].time_base)))
    };
    let demuxer = Arc::new(Demuxer {
        packets: Mutex::new(Packets {
            video: queue(TrackKind::Video),
            audio: queue(TrackKind::Audio),
            // A file's subtitles don't come from the demuxer, and are still
            // being read.
            subtitle: decoders
                .subtitle
                .as_ref()
                .map(|s| PacketQueue::new(s.index, f64::from(decoders.streams[s.index].time_base))),
            spare: decoders
                .streams
                .iter()
                .filter(|s| s.parameters.medium() == media::Type::Subtitle)
                .map(|s| PacketQueue::new(s.index, f64::from(s.time_base)))
                .collect(),
            cover_index: cover.map(|c| c.index),
            cover: None,
            cover_seen: false,
            bytes: 0,
            serial,
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
        let name = name.to_owned();
        thread::Builder::new()
            .name(format!("demux {name}"))
            .spawn(move || {
                guarded(&demuxer.shared, &name, || {
                    demux(input, &demuxer, &name);
                    Ok(())
                });
            })
            .context("starting the demux thread")?;
    }
    Ok(demuxer)
}

/// The commands sent since the decode thread last looked.
struct Received {
    /// The latest seek, as `(time, serial, mode)`.
    seek: Option<(f64, u64, SeekMode)>,
    /// Each pick of a track, with its number.
    selects: Vec<(TrackKind, Option<usize>, u64)>,
}

/// Take the commands waiting, or `None` once the decode thread should stop.
/// Seeks are coalesced so dragging a slider only seeks to the latest spot.
fn receive(commands: &Receiver<Command>) -> Option<Received> {
    let mut seek = None;
    let mut selects = Vec::new();
    let first = match commands.try_recv() {
        Ok(command) => Some(command),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => return None,
    };
    if let Some(mut command) = first {
        loop {
            match command {
                Command::Stop => return None,
                Command::Seek { time, serial, mode } => seek = Some((time, serial, mode)),
                Command::Select { kind, index, pick } => selects.push((kind, index, pick)),
            }
            command = match commands.try_recv() {
                Ok(next) => next,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return None,
            };
        }
    }
    Some(Received { seek, selects })
}

/// Seek to `time`, or for a scrub a little ahead of where decoding is,
/// decode on to it.
fn seek_to(
    session: &mut Session,
    decoders: &mut Decoders,
    demuxer: &Demuxer,
    time: f64,
    serial: u64,
    mode: SeekMode,
) {
    let shared = session.shared;
    let scrub = mode == SeekMode::Scrub;
    // A scrub a little ahead of where decoding is gets there sooner
    // decoding on than from a keyframe.
    let decode_on = scrub
        && session.scrub
        && session.settled
        && decoders
            .video
            .as_ref()
            .is_some_and(|v| v.reaches(time, session.seek_target));
    if decode_on && demuxer.renumber(serial) {
        if let Some(v) = &mut decoders.video {
            v.retarget();
        }
    } else {
        let target = ((time + session.start_offset()).max(0.0) * AV_TIME_BASE) as i64;
        demuxer.seek(Seek {
            target,
            serial,
            mode,
        });
        let cue_time = shared.subtitles().cue_time(time);
        decoders.reset(cue_time);
        session.settled = false;
    }
    session.serial = serial;
    session.seek_target = Some(time);
    session.scrub = scrub;
    shared.reset_queues(serial);
}

/// Settle the last seek once the demux thread says where it landed. A
/// skip may land on a keyframe away from where it was asked to; its
/// packets wait until that's known.
fn settle(session: &mut Session, decoders: &mut Decoders, demuxer: &Demuxer) {
    let (shared, name) = (session.shared, session.name);
    if !session.settled
        && let Some(landed) = demuxer.landed(session.serial)
    {
        session.settled = true;
        if let Some(keyframe) = landed {
            let time = keyframe - session.start_offset();
            log::debug!("{name}: skipped to the keyframe at {time:.2}s");
            session.seek_target = Some(time);
            shared.set_landed(session.serial, time);
            if let Some(file) = &mut decoders.subtitle_file {
                file.seek(shared.subtitles().cue_time(time));
            }
        }
    }
}

/// Decode a video packet if the frames queued have room; a scrub stops at
/// the frame asked for. Says whether it got anywhere.
fn decode_video(session: &mut Session, decoders: &mut Decoders, demuxer: &Demuxer) -> bool {
    let shared = session.shared;
    let mut progressed = false;
    let video_room = session.settled
        && decoders.video.as_ref().is_some_and(|v| {
            !v.finished
                && !(session.scrub && v.reached)
                && shared.queued_frames() < v.max_queued_frames()
        });
    if video_room && let Some(v) = &mut decoders.video {
        match demuxer.pop(TrackKind::Video, session.serial) {
            Next::Packet(packet) => {
                session.start_offset = session
                    .start_offset
                    .or_else(|| packet_time(&packet, v.time_base));
                v.decode(&packet, session);
                progressed = true;
            }
            Next::End => {
                v.finish(session);
                progressed = true;
            }
            Next::Later => shared.set_video_ended(session.serial),
            Next::Wait => {}
        }
    }
    progressed
}

/// Decode an audio packet if the sound queued has room; a scrub is silent.
/// Says whether it got anywhere.
fn decode_audio(session: &mut Session, decoders: &mut Decoders, demuxer: &Demuxer) -> bool {
    if session.scrub {
        return drop_audio(session, demuxer);
    }
    let mut progressed = false;
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
                a.decode(&packet, session);
                progressed = true;
            }
            Next::End => {
                a.finish(session);
                progressed = true;
            }
            Next::Later => session.shared.set_audio_ended(session.serial),
            Next::Wait => {}
        }
    }
    progressed
}

/// Drop an audio packet read while scrubbing: a scrub is silent, and ends
/// in a seek that reads it again. Kept, the sound would fill what can be
/// queued, and the picture would be read no further. Says whether it did.
fn drop_audio(session: &Session, demuxer: &Demuxer) -> bool {
    matches!(
        demuxer.pop(TrackKind::Audio, session.serial),
        Next::Packet(_)
    )
}

/// Subtitles are few and small, and each has to be ready before its
/// time, so they are decoded as soon as they are read. A subtitle
/// file is read all at once, so it is decoded as the playhead nears.
/// Says whether any were.
fn decode_subtitles(session: &Session, decoders: &mut Decoders, demuxer: &Demuxer) -> bool {
    let shared = session.shared;
    let mut progressed = decoders.take_read(demuxer, shared, session.name);
    if let Some(s) = &mut decoders.subtitle
        && session.settled
    {
        match &mut decoders.subtitle_file {
            Some(file) => {
                let now = shared.clock().now();
                let until = shared.subtitles().cue_time(now) + SUBTITLE_FILE_AHEAD;
                while let Some(packet) = file.next(until) {
                    progressed = true;
                    // Its times start with the video's.
                    queue_cue(s.decode(packet, 0.0), session);
                }
            }
            None => {
                while let Next::Packet(packet) = demuxer.pop(TrackKind::Subtitle, session.serial) {
                    progressed = true;
                    queue_cue(s.decode(&packet, session.start_offset()), session);
                }
            }
        }
    }
    progressed
}

/// Add a decoded cue to those around the playhead. Those over before a
/// seek's target are kept too, for going back.
fn queue_cue(cue: Option<Cue>, session: &Session) {
    let Some(cue) = cue else {
        return;
    };
    let now = session.shared.clock().now();
    let mut cues = session.shared.subtitles();
    cues.prune(now);
    cues.push(cue);
}

fn rational(r: Rational) -> Option<f64> {
    (r.numerator() > 0 && r.denominator() > 0).then(|| f64::from(r))
}

fn codec_name(id: codec::Id) -> String {
    format!("{id:?}").to_lowercase()
}
