use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use gpui::RenderImage;

use super::audio::Mixer;
use super::decoder::{self, Command, DecodeOptions, MediaInfo, SeekMode, Shared, VideoFrame};
use super::picking::Wanted;
use super::picture::Picture;
use super::source::Source;
use super::subtitles::CueContent;
use super::tracks::TrackKind;

/// Network streams start (and resume after running dry) once this many
/// seconds have been read ahead...
const STREAM_START_SECONDS: f64 = 1.0;
/// ...and this much audio is decoded.
const STREAM_START_AUDIO_SECONDS: f64 = 0.3;
/// Start anyway after this long if there is something to play, in case the
/// read-ahead estimate is off (timestamps jumping in a live stream, say).
const STREAM_MAX_WAIT: Duration = Duration::from_secs(3);
/// How far back the console's frame rate and lateness look.
const RECENT: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct PlayerOptions {
    pub autoplay: bool,
    pub looping: bool,
    pub volume: f32,
    pub max_short_side: Option<u32>,
    pub hardware_decoding: bool,
    /// The audio and subtitles to play as it opens.
    pub wanted: Wanted,
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
/// A picture following sound heard elsewhere seeks there once it is this
/// far off, in seconds; nearer, its clock eases there.
const FOLLOW_SEEK: f64 = 0.5;
/// A still picture following still sound seeks there once it is this far
/// off, in seconds, about a frame: a paused clock doesn't ease.
const FOLLOW_STILL: f64 = 0.04;

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
    /// The last seek is a scrub: whatever it has decoded shows.
    scrubbing: bool,
    buffering_since: Instant,
    network: bool,
    ended: bool,
    looping: bool,
    muted: bool,
    volume: f32,
    stats: Stats,
    recent: Recent,
}

/// How playback is going, for the console.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaybackStats {
    /// Frames shown a second, over the last second.
    pub shown_fps: f64,
    /// Frames shown since the file opened.
    pub shown: u64,
    /// Frames decoded but never shown, since the file opened.
    pub dropped: u64,
    /// How late frames have been shown against the clock lately, on
    /// average, in seconds; early ones count against it.
    pub lateness: Option<f64>,
    /// Frames decoded and waiting to be shown.
    pub queued_frames: usize,
    /// Seconds of sound decoded and waiting to be heard.
    pub queued_audio: f64,
    /// Seconds read ahead of the playhead; infinite once all of it is.
    pub read_ahead: f64,
}

/// The frames shown in the last `RECENT`, and how late each was.
#[derive(Default)]
struct Recent {
    shown: VecDeque<(Instant, f64)>,
    total: u64,
    dropped: u64,
}

impl Recent {
    fn record(&mut self, lateness: f64, dropped: u32) {
        let now = Instant::now();
        while self
            .shown
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > RECENT)
        {
            self.shown.pop_front();
        }
        self.shown.push_back((now, lateness));
        self.total += 1;
        self.dropped += u64::from(dropped);
    }

    /// Frames a second and their mean lateness, over the last `RECENT`.
    fn rate(&self) -> (f64, Option<f64>) {
        let lately: Vec<f64> = self
            .shown
            .iter()
            .filter(|(at, _)| at.elapsed() <= RECENT)
            .map(|(_, lateness)| *lateness)
            .collect();
        let count = lately.len() as f64;
        let lateness = (!lately.is_empty()).then(|| lately.iter().sum::<f64>() / count);
        (count / RECENT.as_secs_f64(), lateness)
    }
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
        let shared = Arc::new(Shared::new(
            mixer.as_ref().map(|mixer| mixer.sample_rate()),
            options.volume,
        ));
        if let (Some(mixer), Some(audio)) = (&mixer, shared.audio()) {
            mixer.add_source(audio.clone());
        }
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
            options.wanted,
            receiver,
        );
        Self {
            source,
            stats: Stats::new(name),
            recent: Recent::default(),
            network,
            shared,
            commands,
            mixer,
            current: None,
            playing: options.autoplay,
            buffering: true,
            scrubbing: false,
            buffering_since: Instant::now(),
            ended: false,
            looping: options.looping,
            muted: false,
            volume: options.volume,
        }
    }

    /// What the file holds, once it is known: shared, not copied.
    pub fn info(&self) -> Option<Arc<MediaInfo>> {
        self.shared.info()
    }

    /// Whether it is known yet what the file holds.
    pub fn is_known(&self) -> bool {
        self.shared.info().is_some()
    }

    pub fn has_video(&self) -> bool {
        self.shared.info().is_some_and(|i| i.has_video)
    }

    pub fn has_audio(&self) -> bool {
        self.shared.info().is_some_and(|i| i.has_audio)
    }

    /// How long until the next frame decoded is due, while it plays: for
    /// what ticks it on its own, to know when to again.
    pub fn until_next_frame(&self) -> Option<Duration> {
        if !self.playing || self.buffering {
            return None;
        }
        let frames = self.shared.frames()?;
        let next = frames.front()?;
        let left = next.pts - self.shared.clock().now();
        Some(Duration::from_secs_f64(left.max(0.0)))
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error()
    }

    /// Say why it can't play, as a decoder thread that fails does, unless
    /// one has already: nothing more is shown.
    pub fn fail(&mut self, why: String) {
        self.shared.fail(why);
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
        self.shared.cover()
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
        self.shared.info().is_some_and(|i| i.live)
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

    /// Play into `mixer` from now on, with what it has queued, as its
    /// window plays through another output.
    pub fn set_mixer(&mut self, mixer: Option<Arc<Mixer>>) {
        let same = match (&self.mixer, &mixer) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (old, new) => old.is_none() && new.is_none(),
        };
        if same {
            return;
        }
        if let Some(audio) = self.shared.audio() {
            if let Some(old) = &self.mixer {
                old.remove_source(audio);
            }
            if let Some(new) = &mixer {
                new.add_source(audio.clone());
            }
        }
        self.mixer = mixer;
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn duration(&self) -> Option<f64> {
        self.shared.info().and_then(|i| i.duration)
    }

    pub fn position(&self) -> f64 {
        let now = self.shared.clock().now().max(0.0);
        match self.duration() {
            Some(duration) => now.min(duration),
            None => now,
        }
    }

    /// Current audio level, 0..1.
    pub fn level(&self) -> f32 {
        self.shared.audio().map_or(0.0, |a| a.peak.load())
    }

    /// It played through to its end and stopped there, not looping:
    /// moving it anywhere, its end too, starts it over.
    pub fn has_played_out(&self) -> bool {
        self.ended
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

    /// Seek to exactly `time`.
    pub fn seek(&mut self, time: f64) {
        self.seek_to(time, SeekMode::Exact);
    }

    /// Show `time` while the slider is dragged: frames on the way show as
    /// they are decoded, then the one asked for, with nothing read ahead and
    /// no sound. A `seek` once it's let go plays on from there.
    pub fn scrub(&mut self, time: f64) {
        self.seek_to(time, SeekMode::Scrub);
    }

    fn seek_to(&mut self, time: f64, mode: SeekMode) {
        if self.is_live() {
            return;
        }
        let time = match self.duration() {
            Some(duration) => time.clamp(0.0, duration),
            None => time.max(0.0),
        };
        let serial = self.shared.new_serial();
        self.ended = false;
        // A scrub waits for nothing: it has no sound, and plays on only
        // after a seek.
        self.scrubbing = mode == SeekMode::Scrub;
        if self.scrubbing {
            self.buffering = false;
            self.apply_running();
        } else {
            self.start_buffering();
        }
        let from = self.shared.clock().now();
        self.shared.subtitles().seeked(from, time);
        self.shared.clock().set(time);
        self.send(Command::Seek { time, serial, mode });
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
        // Shown in menus right away; the decoder puts it back if the
        // track can't be played.
        let Some(pick) = self.shared.select_track(kind, index) else {
            return;
        };
        self.send(Command::Select { kind, index, pick });
        if index.is_none() {
            self.shared.subtitles().clear();
        } else if kind != TrackKind::Subtitle && !self.is_live() {
            // Subtitles start with the packets kept of them, so the picture
            // plays on; the others start at the playhead. Live streams can't
            // seek; theirs starts with the next packet.
            self.seek(self.position());
        }
    }

    /// Tell the decode thread, waking it.
    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
        self.shared.ring();
    }

    /// How many times the decode thread has waited for something to do.
    #[cfg(test)]
    pub(super) fn decoder_waits(&self) -> u64 {
        self.shared.waits()
    }

    /// The seek generation, for tests to tell whether something seeked.
    #[cfg(test)]
    pub(super) fn serial(&self) -> u64 {
        self.shared.serial()
    }

    /// The times of the frames decoded for the last seek and not yet shown.
    #[cfg(test)]
    pub(super) fn queued(&self) -> Vec<f64> {
        let Some(frames) = self.shared.frames() else {
            return Vec::new();
        };
        frames.iter().map(|frame| frame.pts).collect()
    }

    /// The subtitles showing now.
    /// Seeks keep the cues decoded around the playhead, so going back a
    /// frame keeps them on screen.
    pub fn subtitles(&self) -> Vec<Arc<CueContent>> {
        let now = self.shared.clock().now();
        self.shared.subtitles().showing(now)
    }

    /// Skip some seconds forward or back. Where keyframes are far apart, it
    /// lands on the nearest one its way, which shows at once, rather than
    /// decoding everything up to the time asked for.
    pub fn seek_by(&mut self, delta: f64) {
        let forward = delta > 0.0;
        self.seek_to(self.position() + delta, SeekMode::Skip { forward });
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(audio) = self.shared.audio() {
            audio.muted.store(muted, Ordering::Relaxed);
        }
    }

    /// How much later than the picture the sound is heard, in seconds
    /// (negative: earlier).
    pub fn audio_delay(&self) -> f64 {
        self.shared
            .audio()
            .map_or(0.0, |audio| f64::from(audio.delay.load()))
    }

    /// Hear the sound `delay` seconds after the picture (negative: before
    /// it), up to `MAX_AUDIO_DELAY` either way. The sound plays on as it
    /// was; the picture, and the playhead with it, moves against it.
    pub fn set_audio_delay(&mut self, delay: f64) {
        if let Some(audio) = self.shared.audio() {
            let delay = delay.clamp(-MAX_AUDIO_DELAY, MAX_AUDIO_DELAY);
            audio.delay.store(delay as f32);
        }
    }

    /// Hear the sound elsewhere, on a device a cast sends it to, or here
    /// again. While it's elsewhere, the picture follows it (`follow`).
    pub fn set_sound_elsewhere(&mut self, elsewhere: bool) {
        if let Some(audio) = self.shared.audio() {
            audio.elsewhere.store(elsewhere, Ordering::Relaxed);
        }
    }

    /// Whether the sound is heard elsewhere.
    #[cfg(test)]
    pub fn is_sound_elsewhere(&self) -> bool {
        self.shared
            .audio()
            .is_some_and(|audio| audio.elsewhere.load(Ordering::Relaxed))
    }

    /// Keep the picture with sound heard elsewhere, now at `time`: as the
    /// audio delay says, eased there, or sought where it's far off or
    /// both are still.
    pub fn follow(&mut self, time: f64) {
        let time = time + self.audio_delay();
        let off = (time - self.position()).abs();
        if off > FOLLOW_SEEK || (!self.is_playing() && off > FOLLOW_STILL) {
            self.seek(time);
        } else {
            self.shared.clock().ease_to(time, Instant::now());
        }
    }

    /// How much later than their times subtitles show, in seconds
    /// (negative: earlier).
    pub fn subtitle_delay(&self) -> f64 {
        self.shared.subtitles().delay
    }

    /// Show subtitles `delay` seconds after their times (negative: before),
    /// up to `MAX_SUBTITLE_DELAY` either way.
    pub fn set_subtitle_delay(&mut self, delay: f64) {
        self.shared.subtitles().delay = delay.clamp(-MAX_SUBTITLE_DELAY, MAX_SUBTITLE_DELAY);
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(audio) = self.shared.audio() {
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
            self.shared.clock().start();
        } else {
            self.shared.clock().pause();
        }
        if let Some(audio) = self.shared.audio() {
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
        // The decoder stopped on an error: nothing more comes.
        if self.error().is_some() {
            return tick;
        }
        let serial = self.shared.serial();
        // A skip landed on a keyframe away from where it was asked to.
        if let Some(time) = self.shared.take_landed() {
            self.shared.clock().set(time);
        }
        let eof = self.shared.eof();
        let has_audio = info.has_audio && self.shared.audio().is_some();
        let running = self.playing && !self.buffering;
        let stale = self.current.as_ref().is_none_or(|f| f.serial != serial);

        let mut next = None;
        let mut queued_frames = None;
        if let Some(mut frames) = self.shared.frames() {
            if self.scrubbing {
                // A scrub queues only what is to be seen: the latest.
                next = frames.drain(..).next_back();
            } else if stale {
                // After opening or seeking, show the first frame right
                // away.
                next = frames.pop_front();
            }
            if running {
                let now = self.shared.clock().now();
                let mut popped = 0;
                while frames.front().is_some_and(|f| f.pts <= now) {
                    next = frames.pop_front();
                    popped += 1;
                }
                if let Some(frame) = next.as_ref().filter(|_| popped > 0) {
                    self.stats.record(now - frame.pts, popped - 1);
                    self.recent.record(now - frame.pts, popped - 1);
                }
            }
            queued_frames = Some(frames.len());
        }
        if let Some(frame) = next {
            tick.retired = self.current.replace(frame).map(|f| f.picture);
            // Room for another.
            self.shared.ring();
        }
        let video_drained = queued_frames == Some(0);
        let current_pts = self
            .current
            .as_ref()
            .filter(|f| f.serial == serial)
            .map(|f| f.pts);

        if self.buffering {
            // Files play as soon as there is something to play; network
            // streams wait for a little more so they don't stutter. A
            // stream that has ended has nothing more to wait for, though
            // the other plays on: past where the sound ends, or the picture.
            let video_ready = !info.has_video
                || eof
                || self.shared.video_ended()
                || (current_pts.is_some() && (!self.network || queued_frames > Some(0)));
            let audio_ready = !has_audio
                || eof
                || self.shared.audio_ended()
                || if self.network {
                    self.shared.queued_audio() >= STREAM_START_AUDIO_SECONDS
                } else {
                    self.shared.queued_audio() > 0.0
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
            && (!has_audio || self.shared.queued_audio() == 0.0)
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
            // instead of playing on in silence with a frozen picture. Once
            // the sound has ended, the picture tells.
            let starved = if has_audio && !self.shared.audio_ended() {
                self.shared.queued_audio() == 0.0
            } else {
                let frame_time = info.frame_rate.map_or(0.04, |fps| 1.0 / fps);
                video_drained
                    && current_pts.is_some_and(|pts| {
                        self.shared.clock().now() - pts > (2.0 * frame_time).max(0.5)
                    })
            };
            if starved {
                log::debug!("{}: stream ran dry, buffering", self.stats.name);
                self.start_buffering();
            }
        }

        tick.animate = self.playing || self.buffering || self.scrubbing;
        tick
    }

    /// How playback is going, for the console.
    pub fn stats(&self) -> PlaybackStats {
        let (shown_fps, lateness) = self.recent.rate();
        let queued_frames = self.shared.frames().map_or(0, |frames| frames.len());
        let queued_audio = self.shared.queued_audio();
        PlaybackStats {
            shown_fps,
            shown: self.recent.total,
            dropped: self.recent.dropped,
            lateness,
            queued_frames,
            queued_audio,
            read_ahead: self.shared.read_ahead(),
        }
    }

    /// Take every picture this player may have drawn, so they can be released.
    pub fn take_pictures(&mut self) -> Vec<Picture> {
        let mut pictures: Vec<_> = self.current.take().map(|f| f.picture).into_iter().collect();
        pictures.extend(self.shared.take_cover().map(Picture::Image));
        pictures
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Aborts a network connection or read that is still blocking.
        self.shared.stop();
        self.send(Command::Stop);
        if let (Some(mixer), Some(audio)) = (&self.mixer, self.shared.audio()) {
            mixer.remove_source(audio);
        }
    }
}
