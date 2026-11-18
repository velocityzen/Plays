use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use gpui::RenderImage;
use parking_lot::Mutex;

use super::audio::{AudioSource, Mixer};
use super::clock::Clock;
use super::decoder::{self, Command, DecodeOptions, MediaInfo, Shared, VideoFrame, VideoQueue};
use super::picture::Picture;
use super::source::Source;
use super::subtitles::{CueContent, CueQueue};
use super::tracks::TrackKind;

/// Network streams start (and resume after running dry) once this many
/// seconds have been read ahead...
const STREAM_START_SECONDS: f64 = 1.0;
/// ...and this much audio is decoded.
const STREAM_START_AUDIO_SECONDS: f64 = 0.3;
/// Start anyway after this long if there is something to play, in case the
/// read-ahead estimate is off (timestamps jumping in a live stream, say).
const STREAM_MAX_WAIT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy)]
pub struct PlayerOptions {
    pub autoplay: bool,
    pub looping: bool,
    pub volume: f32,
    pub max_short_side: Option<u32>,
    pub hardware_decoding: bool,
}

/// What the view needs to do after advancing the player.
#[derive(Default)]
pub struct Tick {
    /// The frame that was on screen until now. If it was drawn, the GPU may
    /// still be using it (and an image is in gpui's atlas until released).
    pub retired: Option<Picture>,
    /// Keep redrawing every display frame.
    pub animate: bool,
}

/// UI-thread handle to one playing file.
/// The furthest the sound can be moved from the picture. The demuxer reads
/// that much further ahead for the stream that is behind.
pub const MAX_AUDIO_DELAY: f64 = 5.0;
/// The furthest subtitles can be moved from their times.
pub const MAX_SUBTITLE_DELAY: f64 = 60.0;

pub struct Player {
    source: Source,
    shared: Arc<Shared>,
    commands: Sender<Command>,
    mixer: Option<Arc<Mixer>>,
    current: Option<VideoFrame>,
    /// The user wants playback to run.
    playing: bool,
    /// Waiting for data after opening, seeking or a network stall; the clock is held.
    buffering: bool,
    buffering_since: Instant,
    network: bool,
    ended: bool,
    looping: bool,
    muted: bool,
    volume: f32,
    stats: Stats,
}

/// Debug-level playback statistics, logged every few seconds while playing.
struct Stats {
    name: String,
    since: Instant,
    shown: u32,
    dropped: u32,
    /// Sum of (clock - pts) for shown frames, to spot A/V drift.
    lateness: f64,
}

impl Stats {
    fn record(&mut self, lateness: f64, dropped: u32) {
        self.shown += 1;
        self.dropped += dropped;
        self.lateness += lateness;
        let elapsed = self.since.elapsed().as_secs_f64();
        if elapsed >= 5.0 {
            log::debug!(
                "{}: {:.1} fps shown, {} dropped, video {:+.1} ms vs clock",
                self.name,
                self.shown as f64 / elapsed,
                self.dropped,
                self.lateness / self.shown as f64 * 1000.0
            );
            *self = Stats::new(std::mem::take(&mut self.name));
        }
    }

    fn new(name: String) -> Self {
        Self {
            name,
            since: Instant::now(),
            shown: 0,
            dropped: 0,
            lateness: 0.0,
        }
    }
}

impl Player {
    pub fn open(source: Source, mixer: Option<Arc<Mixer>>, options: PlayerOptions) -> Self {
        let clock = Arc::new(Clock::new());
        let serial = Arc::new(AtomicU64::new(1));
        let audio = mixer.as_ref().map(|mixer| {
            let source = Arc::new(AudioSource::new(
                clock.clone(),
                serial.clone(),
                mixer.sample_rate,
                options.volume,
            ));
            mixer.add_source(source.clone());
            source
        });
        let shared = Arc::new(Shared {
            clock,
            serial,
            info: Mutex::new(None),
            error: Mutex::new(None),
            video: Mutex::new(VideoQueue::default()),
            audio,
            cover: Mutex::new(None),
            subtitles: Mutex::new(CueQueue::default()),
            eof_serial: AtomicU64::new(0),
            read_ahead: AtomicU64::new(0.0f64.to_bits()),
            stop: AtomicBool::new(false),
        });
        let name = source.name();
        let network = source.is_network();
        let (commands, receiver) = mpsc::channel();
        decoder::spawn(
            source.clone(),
            shared.clone(),
            DecodeOptions {
                max_short_side: options.max_short_side,
                hardware: options.hardware_decoding,
            },
            receiver,
        );
        Self {
            source,
            stats: Stats::new(name),
            network,
            shared,
            commands,
            mixer,
            current: None,
            playing: options.autoplay,
            buffering: true,
            buffering_since: Instant::now(),
            ended: false,
            looping: options.looping,
            muted: false,
            volume: options.volume,
        }
    }

    pub fn info(&self) -> Option<MediaInfo> {
        self.shared.info.lock().clone()
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().clone()
    }

    pub fn frame(&self) -> Option<Picture> {
        self.current.as_ref().map(|f| f.picture.clone())
    }

    /// When the frame on screen shows, on the media's timeline.
    pub fn frame_pts(&self) -> Option<f64> {
        self.current.as_ref().map(|f| f.pts)
    }

    /// When the frame on screen shows in its stream's time base, to decode
    /// it again (`still`).
    pub fn frame_timestamp(&self) -> Option<i64> {
        self.current.as_ref().and_then(|f| f.timestamp)
    }

    /// What is playing.
    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn cover(&self) -> Option<Arc<RenderImage>> {
        self.shared.cover.lock().clone()
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Waiting for data before (re)starting playback.
    pub fn is_buffering(&self) -> bool {
        self.buffering && self.error().is_none()
    }

    pub fn is_network(&self) -> bool {
        self.network
    }

    /// A live network stream, which has no duration and can't seek.
    pub fn is_live(&self) -> bool {
        self.shared.info.lock().as_ref().is_some_and(|i| i.live)
    }

    pub fn is_looping(&self) -> bool {
        self.looping
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// The mixer its audio goes to, for opening another file in its place.
    pub fn mixer(&self) -> Option<Arc<Mixer>> {
        self.mixer.clone()
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn duration(&self) -> Option<f64> {
        self.shared.info.lock().as_ref().and_then(|i| i.duration)
    }

    pub fn position(&self) -> f64 {
        let now = self.shared.clock.now().max(0.0);
        match self.duration() {
            Some(duration) => now.min(duration),
            None => now,
        }
    }

    /// Current audio level, 0..1.
    pub fn level(&self) -> f32 {
        self.shared.audio.as_ref().map_or(0.0, |a| a.peak.load())
    }

    pub fn play(&mut self) {
        if self.ended {
            self.seek(0.0);
        }
        self.playing = true;
        self.apply_running();
    }

    pub fn pause(&mut self) {
        self.playing = false;
        self.apply_running();
    }

    pub fn toggle(&mut self) {
        if self.playing {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn seek(&mut self, time: f64) {
        if self.is_live() {
            return;
        }
        let time = match self.duration() {
            Some(duration) => time.clamp(0.0, duration),
            None => time.max(0.0),
        };
        let serial = self.shared.serial.fetch_add(1, Ordering::AcqRel) + 1;
        self.ended = false;
        self.start_buffering();
        self.shared.clock.set(time);
        let _ = self.commands.send(Command::Seek { time, serial });
    }

    /// Decode the frame on screen again, for a paused player whose frames
    /// are now converted differently (a new gamma). It stays on screen
    /// until the new one is ready.
    pub fn refresh(&mut self) {
        if let Some(pts) = self.current.as_ref().map(|frame| frame.pts) {
            self.seek(pts);
        }
    }

    /// Play another track, or turn subtitles off (`None`). The new track is
    /// read from the playhead; until it is, the picture on screen stays.
    pub fn select_track(&mut self, kind: TrackKind, index: Option<usize>) {
        {
            let mut info = self.shared.info.lock();
            let Some(info) = info.as_mut().filter(|info| info.selected(kind) != index) else {
                return;
            };
            // Shown in menus right away; the decoder puts it back if the
            // track can't be played.
            info.select(kind, index);
        }
        let _ = self.commands.send(Command::Select { kind, index });
        if index.is_none() {
            let mut cues = self.shared.subtitles.lock();
            let serial = cues.serial;
            cues.clear(serial);
        } else if !self.is_live() {
            // Live streams can't seek; theirs starts with the next packet.
            self.seek(self.position());
        }
    }

    /// The subtitles showing now.
    pub fn subtitles(&self) -> Vec<Arc<CueContent>> {
        let serial = self.shared.serial.load(Ordering::Acquire);
        let mut cues = self.shared.subtitles.lock();
        if cues.serial != serial {
            return Vec::new();
        }
        cues.showing(self.shared.clock.now())
    }

    pub fn seek_by(&mut self, delta: f64) {
        self.seek(self.position() + delta);
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(audio) = &self.shared.audio {
            audio.muted.store(muted, Ordering::Relaxed);
        }
    }

    /// How much later than the picture the sound is heard, in seconds
    /// (negative: earlier).
    pub fn audio_delay(&self) -> f64 {
        self.shared
            .audio
            .as_ref()
            .map_or(0.0, |audio| f64::from(audio.delay.load()))
    }

    /// Hear the sound `delay` seconds after the picture (negative: before
    /// it), up to `MAX_AUDIO_DELAY` either way. The sound plays on as it
    /// was; the picture, and the playhead with it, moves against it.
    pub fn set_audio_delay(&mut self, delay: f64) {
        if let Some(audio) = &self.shared.audio {
            let delay = delay.clamp(-MAX_AUDIO_DELAY, MAX_AUDIO_DELAY);
            audio.delay.store(delay as f32);
        }
    }

    /// How much later than their times subtitles show, in seconds
    /// (negative: earlier).
    pub fn subtitle_delay(&self) -> f64 {
        self.shared.subtitles.lock().delay
    }

    /// Show subtitles `delay` seconds after their times (negative: before),
    /// up to `MAX_SUBTITLE_DELAY` either way.
    pub fn set_subtitle_delay(&mut self, delay: f64) {
        self.shared.subtitles.lock().delay = delay.clamp(-MAX_SUBTITLE_DELAY, MAX_SUBTITLE_DELAY);
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(audio) = &self.shared.audio {
            audio.volume.store(self.volume);
        }
    }

    /// Hold the clock until enough data is ready again.
    fn start_buffering(&mut self) {
        self.buffering = true;
        self.buffering_since = Instant::now();
        self.apply_running();
    }

    /// Start or hold the clock and the audio to match `playing` and `buffering`.
    fn apply_running(&self) {
        let running = self.playing && !self.buffering;
        if running {
            self.shared.clock.start();
        } else {
            self.shared.clock.pause();
        }
        if let Some(audio) = &self.shared.audio {
            audio.playing.store(running, Ordering::Relaxed);
        }
    }

    /// Advance to the frame that should be on screen now. Call once per render.
    pub fn tick(&mut self) -> Tick {
        let mut tick = Tick::default();
        let Some(info) = self.info() else {
            // Still opening, unless it failed.
            tick.animate = self.error().is_none();
            return tick;
        };
        let serial = self.shared.serial.load(Ordering::Acquire);
        let eof = self.shared.eof_serial.load(Ordering::Acquire) == serial;
        let has_audio = info.has_audio && self.shared.audio.is_some();
        let running = self.playing && !self.buffering;
        let stale = self.current.as_ref().is_none_or(|f| f.serial != serial);

        let mut next = None;
        let mut queued_frames = None;
        {
            let mut queue = self.shared.video.lock();
            if queue.serial == serial {
                // After opening or seeking, show the first frame right away.
                if stale {
                    next = queue.frames.pop_front();
                }
                if running {
                    let now = self.shared.clock.now();
                    let mut popped = 0;
                    while queue.frames.front().is_some_and(|f| f.pts <= now) {
                        next = queue.frames.pop_front();
                        popped += 1;
                    }
                    if let Some(frame) = next.as_ref().filter(|_| popped > 0) {
                        self.stats.record(now - frame.pts, popped - 1);
                    }
                }
                queued_frames = Some(queue.frames.len());
            }
        }
        if let Some(frame) = next {
            tick.retired = self.current.replace(frame).map(|f| f.picture);
        }
        let video_drained = queued_frames == Some(0);
        let current_pts = self
            .current
            .as_ref()
            .filter(|f| f.serial == serial)
            .map(|f| f.pts);

        // Seconds of decoded audio for this serial.
        let audio_buffered = || {
            self.shared.audio.as_ref().map_or(0.0, |a| {
                let queue = a.queue.lock();
                if queue.serial == serial {
                    queue.buffered_frames as f64 / a.sample_rate as f64
                } else {
                    0.0
                }
            })
        };

        if self.buffering {
            // Files play as soon as there is something to play; network
            // streams wait for a little more so they don't stutter.
            let video_ready = !info.has_video
                || eof
                || (current_pts.is_some() && (!self.network || queued_frames > Some(0)));
            let audio_ready = !has_audio
                || eof
                || if self.network {
                    audio_buffered() >= STREAM_START_AUDIO_SECONDS
                } else {
                    audio_buffered() > 0.0
                };
            let read_ahead_ready = !self.network
                || eof
                || self.shared.read_ahead() >= STREAM_START_SECONDS
                || self.buffering_since.elapsed() >= STREAM_MAX_WAIT;
            if video_ready && audio_ready && read_ahead_ready {
                self.buffering = false;
                self.apply_running();
            }
        } else if self.playing
            && eof
            && (!info.has_video || video_drained)
            && (!has_audio || audio_buffered() == 0.0)
        {
            if self.looping && !info.live {
                self.seek(0.0);
            } else {
                self.ended = true;
                self.playing = false;
                self.apply_running();
            }
        } else if running && self.network && !eof {
            // A network stream ran dry: hold the clock until data arrives again
            // instead of playing on in silence with a frozen picture.
            let starved = if has_audio {
                audio_buffered() == 0.0
            } else {
                let frame_time = info.frame_rate.map_or(0.04, |fps| 1.0 / fps);
                video_drained
                    && current_pts.is_some_and(|pts| {
                        self.shared.clock.now() - pts > (2.0 * frame_time).max(0.5)
                    })
            };
            if starved {
                log::debug!("{}: stream ran dry, buffering", self.stats.name);
                self.start_buffering();
            }
        }

        tick.animate = self.playing || self.buffering;
        tick
    }

    /// Take every picture this player may have drawn, so they can be released.
    pub fn take_pictures(&mut self) -> Vec<Picture> {
        let mut pictures: Vec<_> = self.current.take().map(|f| f.picture).into_iter().collect();
        pictures.extend(self.shared.cover.lock().take().map(Picture::Image));
        pictures
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Aborts a network connection or read that is still blocking.
        self.shared.stop.store(true, Ordering::Relaxed);
        let _ = self.commands.send(Command::Stop);
        if let (Some(mixer), Some(audio)) = (&self.mixer, &self.shared.audio) {
            mixer.remove_source(audio);
        }
    }
}
