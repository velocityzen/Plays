use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::thread::{self, JoinHandle};

use super::*;

/// The system's allocator, counting what a thread watching allocates
/// and frees.
struct Watched;

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static SEEN: Cell<usize> = const { Cell::new(0) };
}

fn note() {
    let _ = WATCHING.try_with(|watching| {
        if watching.get() {
            SEEN.with(|seen| seen.set(seen.get() + 1));
        }
    });
}

// SAFETY: every call goes on to the system allocator as it came.
unsafe impl GlobalAlloc for Watched {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller's promises about `layout`, passed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: as for `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        // SAFETY: `ptr` came from this allocator, so from `System`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: as for `dealloc`, and the caller's promise about
        // `new_size`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Watched = Watched;

/// How many times `f` allocates or frees on this thread.
fn allocations(f: impl FnOnce()) -> usize {
    SEEN.with(|seen| seen.set(0));
    WATCHING.with(|watching| watching.set(true));
    f();
    WATCHING.with(|watching| watching.set(false));
    SEEN.with(Cell::get)
}

/// A playing player in `mixer`, a tenth of a second queued.
fn playing_in(mixer: &Mixer) -> Arc<AudioSource> {
    let clock = Arc::new(Clock::new());
    clock.start();
    let source = Arc::new(AudioSource::new(
        clock,
        Arc::new(AtomicU64::new(0)),
        mixer.sample_rate(),
        1.0,
    ));
    mixer.add_source(source.clone());
    source.playing.store(true, Ordering::Relaxed);
    source.push(ramp(2400, 0.0, 48_000));
    source.push(ramp(2400, 0.05, 48_000));
    source
}

#[test]
fn mixing_takes_and_lets_go_of_no_memory() {
    let mixer = Mixer::detached(48_000);
    let source = playing_in(&mixer);
    // Through the first chunk, into the second.
    let mut mix = vec![0.0; 3000 * SOURCE_CHANNELS];
    let seen = allocations(|| mixer.mix(&mut mix, 0.0, Instant::now()));
    assert_eq!(seen, 0, "the mixer allocated or freed");
    // Each frame's samples are its index in its chunk.
    let heard = |frame: usize| mix[frame * SOURCE_CHANNELS];
    assert_eq!(
        (heard(2399), heard(2400), heard(2999)),
        (2399.0, 0.0, 599.0)
    );
    assert_eq!(source.queue.lock().buffered_frames, 1800);
    // The one played is let go of by the decoder, as it queues more.
    source.push(ramp(2400, 0.1, 48_000));
    assert_eq!(source.queue.lock().chunks.len(), 2);
}

/// Mix once on another thread; how long it took.
fn mix_elsewhere(mixer: &Arc<Mixer>) -> JoinHandle<Duration> {
    let mixer = mixer.clone();
    thread::spawn(move || {
        let mut mix = vec![0.0; 480 * SOURCE_CHANNELS];
        let start = Instant::now();
        mixer.mix(&mut mix, 0.0, Instant::now());
        start.elapsed()
    })
}

#[test]
fn a_mix_waits_for_no_other_thread() {
    let mixer = Mixer::detached(48_000);
    let source = playing_in(&mixer);
    let hold = Duration::from_millis(200);

    let queue = source.queue.lock();
    let mixing = mix_elsewhere(&mixer);
    thread::sleep(hold);
    drop(queue);
    assert!(mixing.join().unwrap() < hold / 4, "waited for the queue");

    let list = mixer.sources.lock();
    let mixing = mix_elsewhere(&mixer);
    thread::sleep(hold);
    drop(list);
    assert!(mixing.join().unwrap() < hold / 4, "waited for the list");
}

/// `frames` of stereo, each frame's samples its index.
fn ramp(frames: usize, pts: f64, rate: u32) -> AudioChunk {
    AudioChunk {
        pts,
        samples: (0..frames).flat_map(|i| [i as f32; 2]).collect(),
        pos: 0,
        rate,
    }
}

#[test]
fn resamples_linearly() {
    let ramp: Vec<f32> = (0..8).flat_map(|i| [i as f32, -(i as f32)]).collect();
    let up = resample(&ramp, 24_000, 48_000);
    assert_eq!(up.len(), 32);
    assert_eq!(&up[..6], &[0.0, 0.0, 0.5, -0.5, 1.0, -1.0]);
    let down = resample(&ramp, 48_000, 24_000);
    assert_eq!(down, [0.0, 0.0, 2.0, -2.0, 4.0, -4.0, 6.0, -6.0]);
    assert_eq!(resample(&ramp, 48_000, 48_000), ramp);
}

#[test]
fn what_is_queued_follows_the_mixer_to_another_rate() {
    let mut queue = AudioQueue::new(48_000);
    let mut chunk = ramp(4800, 1.0, 48_000);
    chunk.pos = 2400;
    queue.push(chunk);
    assert_eq!(queue.buffered_frames, 2400);
    assert_eq!(queue.read_pts(), Some(1.05));

    queue.set_rate(24_000);
    assert_eq!(queue.rate, 24_000);
    assert_eq!(queue.buffered_frames, 1200);
    assert_eq!(queue.read_pts(), Some(1.05));
    assert!((queue.buffered_seconds() - 0.05).abs() < 1e-9);
    // It goes on from where it was: frame 2400, then every other one.
    let chunk = queue.chunks.front().unwrap();
    assert_eq!(&chunk.samples[..4], &[2400.0, 2400.0, 2402.0, 2402.0]);

    // Decoded at the old rate while it changed: converted on the way in.
    queue.push(ramp(480, 1.1, 48_000));
    assert_eq!(queue.buffered_frames, 1200 + 240);
    assert_eq!(queue.chunks.back().unwrap().rate, 24_000);
}

#[test]
fn sound_heard_elsewhere_is_let_go_as_the_clock_passes_it_unheard() {
    let mixer = Mixer::detached(48_000);
    let clock = Arc::new(Clock::new());
    let source = Arc::new(AudioSource::new(
        clock.clone(),
        Arc::new(AtomicU64::new(0)),
        mixer.sample_rate(),
        1.0,
    ));
    mixer.add_source(source.clone());
    source.playing.store(true, Ordering::Relaxed);
    source.elsewhere.store(true, Ordering::Relaxed);
    source.queue.lock().push(ramp(48_000, 0.0, 48_000));
    clock.set(0.25);
    let mut mix = vec![1.0; 960];
    mixer.mix(&mut mix, 0.0, Instant::now());

    assert!(mix.iter().all(|&sample| sample == 0.0), "unheard");
    assert_eq!(source.peak.load(), 0.0);
    assert_eq!(clock.now(), 0.25, "the clock goes by the sound elsewhere");
    let queue = source.queue.lock();
    assert_eq!(queue.buffered_frames, 36_000, "what played went");
    assert_eq!(queue.read_pts(), Some(0.25));
}

#[test]
fn a_mixer_changes_every_player_to_its_rate() {
    let mixer = Mixer::detached(48_000);
    let source = Arc::new(AudioSource::new(
        Arc::new(Clock::new()),
        Arc::new(AtomicU64::new(0)),
        mixer.sample_rate(),
        1.0,
    ));
    mixer.add_source(source.clone());
    source.queue.lock().push(ramp(4800, 0.0, 48_000));
    mixer.set_sample_rate(44_100);
    assert_eq!(mixer.sample_rate(), 44_100);
    assert_eq!(source.sample_rate(), 44_100);
    assert_eq!(source.queue.lock().buffered_frames, 4410);
}

#[test]
fn a_player_moved_to_another_mixer_plays_at_its_rate() {
    let (here, there) = (Mixer::detached(48_000), Mixer::detached(44_100));
    let source = Arc::new(AudioSource::new(
        Arc::new(Clock::new()),
        Arc::new(AtomicU64::new(0)),
        here.sample_rate(),
        1.0,
    ));
    here.add_source(source.clone());
    source.queue.lock().push(ramp(4800, 0.0, 48_000));

    here.remove_source(&source);
    there.add_source(source.clone());
    assert_eq!(
        source.sample_rate(),
        44_100,
        "what its decoder resamples to"
    );
    assert_eq!(
        source.queue.lock().buffered_frames,
        4410,
        "what it had queued"
    );
    assert_eq!(here.source_count(), 0);
    assert_eq!(there.source_count(), 1);
}

#[test]
fn a_player_twice_as_fast_is_heard_through_its_sound_twice_as_fast() {
    let mixer = Mixer::detached(48_000);
    let source = playing_in(&mixer);
    source.speed.store(2.0);
    // A fiftieth of a second.
    let mut mix = vec![0.0; 960 * SOURCE_CHANNELS];
    let seen = allocations(|| mixer.mix(&mut mix, 0.0, Instant::now()));
    assert_eq!(seen, 0, "the mixer allocated or freed at a speed");
    assert!(mix.iter().any(|&x| x != 0.0), "it's heard");
    let heard = source.queue.lock().read_pts().unwrap();
    assert!((heard - 0.04).abs() < 1e-6, "a twenty-fifth on: {heard}");
    // Back at its own speed, it plays on from there.
    source.speed.store(1.0);
    mixer.mix(&mut mix, 0.0, Instant::now());
    let heard = source.queue.lock().read_pts().unwrap();
    assert!((heard - 0.06).abs() < 1e-6, "a fiftieth further: {heard}");
}
