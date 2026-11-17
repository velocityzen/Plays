//! End-to-end checks of the decoding engine against small files generated
//! with the `ffmpeg` command line tool. Skipped when `ffmpeg` isn't installed.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ffmpeg_next::Rational;

use super::audio::Mixer;
use super::decoder::output_size;
use super::player::{Player, PlayerOptions};

fn options(autoplay: bool, looping: bool) -> PlayerOptions {
    PlayerOptions {
        autoplay,
        looping,
        volume: 1.0,
        max_short_side: None,
    }
}

/// Generate a test file once per test run; `None` if ffmpeg is unavailable.
fn media(name: &str, args: &[&str]) -> Option<PathBuf> {
    ffmpeg_next::init().unwrap();
    let dir = std::env::temp_dir().join("plays-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    if path.exists() {
        return Some(path);
    }
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .arg(&path)
        .status()
        .ok()?;
    status.success().then_some(path)
}

fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

#[test]
fn opens_and_seeks_video_accurately() {
    let Some(path) = media(
        "video.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=4",
            "-c:v",
            "libx264",
            "-g",
            "50",
            "-pix_fmt",
            "yuv420p",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mut player = Player::open(path, None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let info = player.info().unwrap();
    assert!(info.has_video && !info.has_audio);
    assert_eq!(info.video_size, Some((320, 240)));
    assert!(
        (info.duration.unwrap() - 4.0).abs() < 0.1,
        "{:?}",
        info.duration
    );
    assert!(player.frame_pts().unwrap().abs() < 0.01);

    // 2.5 s is mid-GOP, so this has to decode forward from the keyframe at 0.
    for target in [2.5, 0.52, 3.9] {
        player.seek(target);
        assert!(
            wait_until(Duration::from_secs(5), || {
                player.tick();
                player
                    .frame_pts()
                    .is_some_and(|pts| (pts - target).abs() < 0.045)
            }),
            "seek to {target} landed on {:?}",
            player.frame_pts()
        );
    }
}

/// Pull audio through a mixer until the player stops; returns non-silent frames heard.
fn drain_audio(
    player: &mut Player,
    mixer: &Mixer,
    timeout: Duration,
    stop: impl Fn(usize) -> bool,
) -> usize {
    let mut heard = 0;
    let mut buffer = vec![0.0f32; 512 * 2];
    let start = Instant::now();
    while start.elapsed() < timeout {
        player.tick();
        if !player.is_playing() || stop(heard) {
            break;
        }
        mixer.mix(&mut buffer, 0.0, Instant::now());
        heard += buffer
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|f| f[0].abs() > 1e-6)
            .count();
        std::thread::sleep(Duration::from_micros(300));
    }
    heard
}

#[test]
fn resamples_audio_and_stops_at_the_end() {
    let Some(path) = media(
        "tone.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=1",
            "-ac",
            "2",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(path, Some(mixer.clone()), options(true, false));
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |_| false);
    assert!(!player.is_playing(), "player should stop at the end");
    // One second of audio, resampled from 44.1 kHz to 48 kHz.
    assert!((heard as i64 - 48_000).abs() < 480, "heard {heard} frames");
    drop(player);
    let _ = Arc::strong_count(&mixer);
}

#[test]
fn loops_when_asked() {
    let Some(path) = media(
        "short.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=300:sample_rate=48000:duration=0.25",
            "-ac",
            "2",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(path, Some(mixer.clone()), options(true, true));
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |heard| {
        heard > 36_000
    });
    assert!(player.is_playing(), "looping player kept playing");
    assert!(
        heard > 36_000,
        "only heard {heard} frames; the file is 12000 long"
    );
}

#[test]
fn reports_unplayable_files() {
    let path = std::env::temp_dir().join(format!("plays-not-media-{}.txt", std::process::id()));
    std::fs::write(&path, "definitely not a video").unwrap();
    ffmpeg_next::init().unwrap();
    let mut player = Player::open(path.clone(), None, options(true, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.error().is_some()
    }));
    std::fs::remove_file(path).ok();
}

#[test]
fn sizes_frames_for_display() {
    let square = Rational::new(1, 1);
    assert_eq!(output_size(1920, 1080, square, None), (1920, 1080));
    assert_eq!(output_size(3840, 2160, square, Some(1080)), (1920, 1080));
    // Portrait video is capped on its short side too.
    assert_eq!(output_size(1080, 1920, square, Some(720)), (720, 1280));
    // Anamorphic DVD: 720x480 with 32:27 pixels is 853x480 on screen (kept even).
    assert_eq!(
        output_size(720, 480, Rational::new(32, 27), None),
        (852, 480)
    );
    // Unknown aspect ratio is treated as square.
    assert_eq!(output_size(641, 361, Rational::new(0, 1), None), (640, 360));
}

/// With the window hidden, frames are only consumed a few times a second. Large
/// frames fill the queue quickly, so the decoder must skip frames that are
/// already late instead of falling further and further behind.
#[test]
fn keeps_up_when_frames_are_rarely_consumed() {
    let Some(path) = media(
        "big60.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1920x1080:rate=60:duration=4",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mut player = Player::open(path, None, options(true, false));
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(2500) {
        player.tick();
        std::thread::sleep(Duration::from_millis(200));
    }
    player.tick();
    let behind = player.position() - player.frame_pts().unwrap();
    assert!(behind < 0.3, "video is {behind:.2}s behind the clock");
}
