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
            read_ahead: AtomicU64::new(0.0f64.to_bits()),
            stop: AtomicBool::new(false),
            landed: Mutex::new(None),
            wake: Doorbell::default(),
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
    /// the file holds isn't known yet. Whether it changed.
    pub fn select_track(&self, kind: TrackKind, index: Option<usize>) -> bool {
        let mut info = self.info.lock();
        let Some(info) = info.as_mut().filter(|info| info.selected(kind) != index) else {
            return false;
        };
        Arc::make_mut(info).select(kind, index);
        true
    }

    /// Why it can't play, once a decoder thread has failed.
    pub fn error(&self) -> Option<String> {
        self.error.lock().clone()
    }

    /// Say why it can't play, unless a thread that failed first has.
    pub(super) fn fail(&self, why: String) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::audio::AudioChunk;

    const RATE: u32 = 48_000;

    /// A frame for the seek `serial`, its picture a single pixel.
    fn frame(serial: u64, pts: f64) -> VideoFrame {
        let pixel = image::RgbaImage::new(1, 1);
        let image = RenderImage::new(vec![image::Frame::new(pixel)]);
        VideoFrame {
            pts,
            timestamp: None,
            serial,
            picture: Picture::Image(Arc::new(image)),
        }
    }

    /// The times of the frames handed out now.
    fn handed_out(shared: &Shared) -> Option<Vec<f64>> {
        let frames = shared.frames()?;
        Some(frames.iter().map(|frame| frame.pts).collect())
    }

    #[test]
    fn a_frame_queued_before_a_seek_is_never_handed_out_after_it() {
        let shared = Shared::new(None, 1.0);
        assert_eq!(handed_out(&shared), None, "nothing decoded yet");
        let first = shared.serial();
        shared.reset_queues(first);
        shared.push_frame(frame(first, 0.0));
        assert_eq!(handed_out(&shared), Some(vec![0.0]));

        let second = shared.new_serial();
        assert_eq!(
            handed_out(&shared),
            None,
            "the decode thread hasn't got to the seek yet"
        );
        assert_eq!(shared.queued_frames(), 1, "the decode thread's own");

        shared.reset_queues(second);
        // One decoded for the seek before arriving late.
        shared.push_frame(frame(first, 1.0));
        shared.push_frame(frame(second, 2.0));
        assert_eq!(handed_out(&shared), Some(vec![2.0]));
    }

    #[test]
    fn sound_queued_before_a_seek_is_not_counted_after_it() {
        let shared = Shared::new(Some(RATE), 1.0);
        let audio = shared.audio().expect("a mixer's rate was given");
        shared.reset_queues(shared.serial());
        audio.push(AudioChunk {
            pts: 0.0,
            samples: vec![0.0; RATE as usize * 2],
            pos: 0,
            rate: RATE,
        });
        assert_eq!(shared.queued_audio(), 1.0);

        shared.new_serial();
        assert_eq!(shared.queued_audio(), 0.0);
    }

    #[test]
    fn the_sound_has_the_same_serial() {
        let shared = Shared::new(Some(RATE), 1.0);
        let serial = shared.new_serial();
        let audio = shared.audio().expect("a mixer's rate was given");
        assert_eq!(audio.serial.load(Ordering::Acquire), serial);
    }

    #[test]
    fn a_landing_for_an_earlier_seek_is_not_taken() {
        let shared = Shared::new(None, 1.0);
        let first = shared.serial();
        shared.set_landed(first, 3.0);
        let second = shared.new_serial();
        assert_eq!(shared.take_landed(), None);

        shared.set_landed(second, 5.0);
        assert_eq!(shared.take_landed(), Some(5.0));
        assert_eq!(shared.take_landed(), None, "it is taken once");
    }

    #[test]
    fn the_end_of_an_earlier_seek_is_not_the_end() {
        let shared = Shared::new(None, 1.0);
        assert!(!shared.eof(), "nothing decoded yet");
        let first = shared.serial();
        shared.set_eof(first);
        assert!(shared.eof());

        let second = shared.new_serial();
        assert!(!shared.eof(), "the seek after it has just started");
        shared.set_eof(second);
        assert!(shared.eof());
    }

    #[test]
    fn the_first_failure_is_the_one_kept() {
        let shared = Shared::new(None, 1.0);
        assert_eq!(shared.error(), None);
        shared.fail("the demuxer failed".into());
        shared.fail("the decoder failed after it".into());
        assert_eq!(shared.error().as_deref(), Some("the demuxer failed"));
    }
}
