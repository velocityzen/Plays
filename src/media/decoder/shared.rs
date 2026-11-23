//! What a player's UI thread and its decoder threads share. Each seek
//! starts a generation, numbered by a serial; what was decoded or found
//! for an earlier one is never handed out once the next has started, so
//! nothing that takes from here has to check the serial itself.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use gpui::RenderImage;
use parking_lot::{Condvar, MappedMutexGuard, Mutex, MutexGuard};

use super::MediaInfo;
use crate::media::audio::AudioSource;
use crate::media::clock::Clock;
use crate::media::picture::Picture;
use crate::media::subtitles::CueQueue;
use crate::media::tracks::TrackKind;

pub struct VideoFrame {
    pub pts: f64,
    /// When it shows in its stream's own time base, to find it again.
    pub timestamp: Option<i64>,
    pub serial: u64,
    pub picture: Picture,
}

/// The frames decoded ahead, every one for the seek `serial`.
#[derive(Default)]
struct VideoQueue {
    frames: VecDeque<VideoFrame>,
    serial: u64,
}

/// Wakes the decode thread when what it waits for may have come: a
/// command, packets read, a seek carried out, a frame taken off its queue.
#[derive(Default)]
struct Doorbell {
    rung: Mutex<bool>,
    bell: Condvar,
    /// How many times the decode thread waited, for tests to tell it
    /// sleeps.
    #[cfg(test)]
    waits: AtomicU64,
}

impl Doorbell {
    fn ring(&self) {
        *self.rung.lock() = true;
        self.bell.notify_one();
    }

    /// Until it rings, or `timeout` passes. A ring while nobody waited
    /// ends the next wait at once.
    fn wait(&self, timeout: Duration) {
        #[cfg(test)]
        self.waits.fetch_add(1, Ordering::Relaxed);
        let mut rung = self.rung.lock();
        if !*rung {
            self.bell.wait_for(&mut rung, timeout);
        }
        *rung = false;
    }

    #[cfg(test)]
    fn waits(&self) -> u64 {
        self.waits.load(Ordering::Relaxed)
    }
}

/// Everything the UI thread and the decoder threads share for one player.
pub struct Shared {
    clock: Arc<Clock>,
    /// The seek generation the UI wants; frames and samples from older
    /// generations are ignored.
    serial: Arc<AtomicU64>,
    /// Shared rather than copied, as the UI reads it every frame; changed
    /// (rarely) by copying it.
    info: Mutex<Option<Arc<MediaInfo>>>,
    error: Mutex<Option<String>>,
    video: Mutex<VideoQueue>,
    audio: Option<Arc<AudioSource>>,
    /// Embedded cover art (for example in an MP3 or M4A).
    cover: Mutex<Option<Arc<RenderImage>>>,
    /// Subtitle cues decoded ahead of the playhead.
    subtitles: Mutex<CueQueue>,
    /// Set to the current serial once every frame and sample for it has been queued.
    eof_serial: AtomicU64,
    /// Each set to the serial its stream has nothing more for: all of it
    /// queued, or none of it for as far as the demuxer reads ahead. One
    /// stream may end, or stop for a long while, well before the other.
    video_ended: AtomicU64,
    audio_ended: AtomicU64,
    /// Seconds of packets read ahead (`f64` bits), infinite once the demuxer
    /// reached the end. Network streams wait for some before playing.
    read_ahead: AtomicU64,
    /// Set when the player goes away; aborts network reads that are blocking.
    stop: AtomicBool,
    /// Where a skip landed (serial, time) when that isn't where it was
    /// asked to, for the clock.
    landed: Mutex<Option<(u64, f64)>>,
    /// Rung for the decode thread: by the player when it sends a command
    /// or takes a frame, and by the demux thread when it reads.
    wake: Doorbell,
    /// How many tracks of each kind (`pick_slot`) have been picked, the
    /// last pick's number: changed and read with `info` locked.
    picks: [AtomicU64; 3],
}

impl Shared {
    /// For a player that has decoded nothing yet. Its sound is resampled
    /// to `sample_rate`, when there is a mixer to play it, and plays at
    /// `volume`.
    pub fn new(sample_rate: Option<u32>, volume: f32) -> Self {
        let clock = Arc::new(Clock::new());
        let serial = Arc::new(AtomicU64::new(1));
        // The mixer checks what it plays against the serial itself, so the
        // sound has this same one.
        let audio = sample_rate.map(|rate| {
            Arc::new(AudioSource::new(
                clock.clone(),
                serial.clone(),
                rate,
                volume,
            ))
        });
        Self {
            clock,
            serial,
            info: Mutex::new(None),
            error: Mutex::new(None),
            video: Mutex::new(VideoQueue::default()),
            audio,
            cover: Mutex::new(None),
            subtitles: Mutex::new(CueQueue::default()),
            eof_serial: AtomicU64::new(0),
            video_ended: AtomicU64::new(0),
            audio_ended: AtomicU64::new(0),
            read_ahead: AtomicU64::new(0.0f64.to_bits()),
            stop: AtomicBool::new(false),
            landed: Mutex::new(None),
            wake: Doorbell::default(),
            picks: Default::default(),
        }
    }

    /// The playback clock, which the picture follows and the sound nudges.
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The seek generation the UI wants.
    pub fn serial(&self) -> u64 {
        self.serial.load(Ordering::Acquire)
    }

    /// Start a new seek generation, and say its serial. Nothing decoded
    /// for an earlier one is handed out from now on.
    pub fn new_serial(&self) -> u64 {
        self.serial.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// What the file holds, once it is known: shared, not copied.
    pub fn info(&self) -> Option<Arc<MediaInfo>> {
        self.info.lock().clone()
    }

    pub(super) fn set_info(&self, info: MediaInfo) {
        *self.info.lock() = Some(Arc::new(info));
    }

    /// Change what the file holds, on a copy, as anyone may be reading it;
    /// nothing until it is known.
    pub(super) fn update_info(&self, change: impl FnOnce(&mut MediaInfo)) {
        if let Some(info) = self.info.lock().as_mut() {
            change(Arc::make_mut(info));
        }
    }

    /// Say track `index` plays for `kind`, unless it does already or what
    /// the file holds isn't known yet. The pick's number if it changed, to
    /// send the decode thread with (see `played`).
    pub fn select_track(&self, kind: TrackKind, index: Option<usize>) -> Option<u64> {
        let mut info = self.info.lock();
        let info = info.as_mut().filter(|info| info.selected(kind) != index)?;
        Arc::make_mut(info).select(kind, index);
        Some(self.picks[pick_slot(kind)].fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Say track `index` plays for `kind`, as the decode thread finds once
    /// it has done pick number `pick`, unless a later one is to come: that
    /// shows till then. Said of an earlier pick, it would hide the later,
    /// and a pick of what it said would then go unsent.
    pub(super) fn played(&self, kind: TrackKind, index: Option<usize>, pick: u64) {
        let mut info = self.info.lock();
        if self.picks[pick_slot(kind)].load(Ordering::Relaxed) != pick {
            return;
        }
        if let Some(info) = info.as_mut() {
            Arc::make_mut(info).select(kind, index);
        }
    }

    /// Why it can't play, once a decoder thread has failed.
    pub fn error(&self) -> Option<String> {
        self.error.lock().clone()
    }

    /// Say why it can't play, unless a thread that failed first has.
    pub(in crate::media) fn fail(&self, why: String) {
        self.error.lock().get_or_insert(why);
    }

    /// Start the frame and sound queues again for the seek `serial`: what
    /// was queued is let go of.
    pub(super) fn reset_queues(&self, serial: u64) {
        {
            let mut video = self.video.lock();
            video.frames.clear();
            video.serial = serial;
        }
        if let Some(audio) = &self.audio {
            audio.clear(serial);
        }
    }

    /// Queue a decoded frame, unless it is for another seek than the one
    /// the queue was last started for.
    pub(super) fn push_frame(&self, frame: VideoFrame) {
        let mut queue = self.video.lock();
        if frame.serial == queue.serial {
            queue.frames.push_back(frame);
        }
    }

    /// How many frames are queued, for the decode thread to keep the queue
    /// within bounds. They are all its own: for the seek it last started
    /// the queue for.
    pub(super) fn queued_frames(&self) -> usize {
        self.video.lock().frames.len()
    }

    /// The frames decoded for the current seek and not yet taken, held
    /// while taking from them. `None` while the queue is still an earlier
    /// seek's, before the decode thread has started it again.
    pub fn frames(&self) -> Option<MappedMutexGuard<'_, VecDeque<VideoFrame>>> {
        let serial = self.serial();
        MutexGuard::try_map(self.video.lock(), |queue| {
            (queue.serial == serial).then_some(&mut queue.frames)
        })
        .ok()
    }

    /// The player's sound, when there is a mixer to play it.
    pub fn audio(&self) -> Option<&Arc<AudioSource>> {
        self.audio.as_ref()
    }

    /// Seconds of sound decoded for the current seek and not yet heard.
    pub fn queued_audio(&self) -> f64 {
        self.audio.as_ref().map_or(0.0, |audio| {
            let queue = audio.queue.lock();
            if queue.serial == self.serial() {
                queue.buffered_seconds()
            } else {
                0.0
            }
        })
    }

    /// Embedded cover art (for example in an MP3 or M4A).
    pub fn cover(&self) -> Option<Arc<RenderImage>> {
        self.cover.lock().clone()
    }

    /// Show `cover` as the cover art, unless some shows already.
    pub(super) fn set_cover(&self, cover: Arc<RenderImage>) {
        self.cover.lock().get_or_insert(cover);
    }

    /// The cover art, to let go of.
    pub fn take_cover(&self) -> Option<Arc<RenderImage>> {
        self.cover.lock().take()
    }

    /// Subtitle cues decoded ahead of the playhead, held while working on
    /// them. Seeks keep them, so they have no serial to check.
    pub fn subtitles(&self) -> MutexGuard<'_, CueQueue> {
        self.subtitles.lock()
    }

    /// Whether every frame and sample for the current seek has been queued.
    pub fn eof(&self) -> bool {
        self.eof_serial.load(Ordering::Acquire) == self.serial()
    }

    /// Say every frame and sample for the seek `serial` has been queued.
    pub(super) fn set_eof(&self, serial: u64) {
        self.eof_serial.store(serial, Ordering::Release);
    }

    /// Whether every frame for the current seek has been queued, or there
    /// is none for as far as the demuxer reads ahead.
    pub fn video_ended(&self) -> bool {
        self.video_ended.load(Ordering::Acquire) == self.serial()
    }

    /// Say every frame for the seek `serial` has been queued, or there is
    /// none for as far as the demuxer reads ahead.
    pub(super) fn set_video_ended(&self, serial: u64) {
        self.video_ended.store(serial, Ordering::Release);
    }

    /// Whether every sample for the current seek has been queued, or there
    /// is none for as far as the demuxer reads ahead.
    pub fn audio_ended(&self) -> bool {
        self.audio_ended.load(Ordering::Acquire) == self.serial()
    }

    /// Say every sample for the seek `serial` has been queued, or there is
    /// none for as far as the demuxer reads ahead.
    pub(super) fn set_audio_ended(&self, serial: u64) {
        self.audio_ended.store(serial, Ordering::Release);
    }

    /// Seconds of packets read ahead, infinite once the demuxer reached
    /// the end. Network streams wait for some before playing.
    pub fn read_ahead(&self) -> f64 {
        f64::from_bits(self.read_ahead.load(Ordering::Relaxed))
    }

    pub(super) fn set_read_ahead(&self, seconds: f64) {
        self.read_ahead.store(seconds.to_bits(), Ordering::Relaxed);
    }

    /// Stop the decoder threads, as the player goes away; aborts network
    /// reads that are blocking.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub(super) fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Where the current seek landed, when it was a skip that landed away
    /// from where it was asked to, for the clock; once. One for an earlier
    /// seek is let go of.
    pub fn take_landed(&self) -> Option<f64> {
        let serial = self.serial();
        let landed = self.landed.lock().take();
        landed
            .filter(|(landed, _)| *landed == serial)
            .map(|(_, time)| time)
    }

    /// Say the seek `serial`, a skip, landed at `time` rather than where it
    /// was asked to.
    pub(super) fn set_landed(&self, serial: u64, time: f64) {
        *self.landed.lock() = Some((serial, time));
    }

    /// Wake the decode thread: by the player when it sends a command or
    /// takes a frame, and by the demux thread when it reads.
    pub fn ring(&self) {
        self.wake.ring();
    }

    /// Wait, on the decode thread, until it is rung or `timeout` passes. A
    /// ring while it wasn't waiting ends the next wait at once.
    pub(super) fn wait(&self, timeout: Duration) {
        self.wake.wait(timeout);
    }

    /// How many times the decode thread waited, for tests to tell it
    /// sleeps.
    #[cfg(test)]
    pub fn waits(&self) -> u64 {
        self.wake.waits()
    }
}

/// Where picks of `kind` are counted in `Shared::picks`.
fn pick_slot(kind: TrackKind) -> usize {
    match kind {
        TrackKind::Video => 0,
        TrackKind::Audio => 1,
        TrackKind::Subtitle => 2,
    }
}

#[cfg(test)]
#[path = "tests/shared.rs"]
mod tests;
