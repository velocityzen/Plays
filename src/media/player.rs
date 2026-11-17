use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Instant;

use gpui::RenderImage;
use parking_lot::Mutex;

use super::audio::{AudioSource, Mixer};
use super::clock::Clock;
use super::decoder::{self, Command, DecodeOptions, MediaInfo, Shared, VideoFrame, VideoQueue};

#[derive(Clone, Copy)]
pub struct PlayerOptions {
    pub autoplay: bool,
    pub looping: bool,
    pub volume: f32,
    pub max_short_side: Option<u32>,
}

/// What the view needs to do after advancing the player.
#[derive(Default)]
pub struct Tick {
    /// The frame that was on screen until now; it must be released from the GPU atlas.
    pub retired: Option<Arc<RenderImage>>,
    /// Keep redrawing every display frame.
    pub animate: bool,
}

/// UI-thread handle to one playing file.
pub struct Player {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    mixer: Option<Arc<Mixer>>,
    current: Option<VideoFrame>,
    /// The user wants playback to run.
    playing: bool,
    /// Waiting for the decoder after opening or seeking; the clock is held.
    buffering: bool,
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
    pub fn open(path: PathBuf, mixer: Option<Arc<Mixer>>, options: PlayerOptions) -> Self {
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
            eof_serial: AtomicU64::new(0),
        });
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (commands, receiver) = mpsc::channel();
        decoder::spawn(
            path,
            shared.clone(),
            DecodeOptions {
                max_short_side: options.max_short_side,
            },
            receiver,
        );
        Self {
            stats: Stats::new(name),
            shared,
            commands,
            mixer,
            current: None,
            playing: options.autoplay,
            buffering: true,
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

    pub fn frame(&self) -> Option<Arc<RenderImage>> {
        self.current.as_ref().map(|f| f.image.clone())
    }

    #[cfg(test)]
    pub fn frame_pts(&self) -> Option<f64> {
        self.current.as_ref().map(|f| f.pts)
    }

    pub fn cover(&self) -> Option<Arc<RenderImage>> {
        self.shared.cover.lock().clone()
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn is_looping(&self) -> bool {
        self.looping
    }

    pub fn is_muted(&self) -> bool {
        self.muted
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
        let time = match self.duration() {
            Some(duration) => time.clamp(0.0, duration),
            None => time.max(0.0),
        };
        let serial = self.shared.serial.fetch_add(1, Ordering::AcqRel) + 1;
        self.ended = false;
        self.buffering = true;
        self.apply_running();
        self.shared.clock.set(time);
        let _ = self.commands.send(Command::Seek { time, serial });
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

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(audio) = &self.shared.audio {
            audio.volume.store(self.volume);
        }
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
        let video_drained;
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
                video_drained = queue.frames.is_empty();
            } else {
                video_drained = false;
            }
        }
        if let Some(frame) = next {
            tick.retired = self.current.replace(frame).map(|f| f.image);
        }

        let audio_drained = || {
            self.shared.audio.as_ref().is_none_or(|a| {
                let queue = a.queue.lock();
                queue.serial == serial && queue.buffered_frames == 0
            })
        };

        if self.buffering {
            let video_ready =
                !info.has_video || eof || self.current.as_ref().is_some_and(|f| f.serial == serial);
            let audio_ready = !has_audio
                || eof
                || self.shared.audio.as_ref().is_some_and(|a| {
                    let queue = a.queue.lock();
                    queue.serial == serial && queue.buffered_frames > 0
                });
            if video_ready && audio_ready {
                self.buffering = false;
                self.apply_running();
            }
        } else if self.playing
            && eof
            && (!info.has_video || video_drained)
            && (!has_audio || audio_drained())
        {
            if self.looping {
                self.seek(0.0);
            } else {
                self.ended = true;
                self.playing = false;
                self.apply_running();
            }
        }

        tick.animate = self.playing || self.buffering;
        tick
    }

    /// Take every image this player may have drawn, so they can be released.
    pub fn take_images(&mut self) -> Vec<Arc<RenderImage>> {
        let mut images: Vec<_> = self.current.take().map(|f| f.image).into_iter().collect();
        images.extend(self.shared.cover.lock().take());
        images
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let (Some(mixer), Some(audio)) = (&self.mixer, &self.shared.audio) {
            mixer.remove_source(audio);
        }
    }
}
