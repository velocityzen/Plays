//! End-to-end checks of the decoding engine against small files generated
//! with the `ffmpeg` command line tool. Skipped when `ffmpeg` isn't installed.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ffmpeg_next::Rational;

use super::audio::Mixer;
use super::decoder::output_size;
use super::orientation::Orientation;
use super::output::{self, AudioOutput};
use super::player::{Player, PlayerOptions};
use super::source::Source;
use super::tracks::TrackKind;

fn options(autoplay: bool, looping: bool) -> PlayerOptions {
    PlayerOptions {
        autoplay,
        looping,
        volume: 1.0,
        max_short_side: None,
        hardware_decoding: false,
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

/// A decoder thread failing as a bug would, once the file is open: the tile
/// says it can't play it, and stops waiting for more.
#[test]
fn a_decoder_thread_failing_says_so_and_stops_waiting() {
    let dir = std::env::temp_dir().join("plays-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let path = dir.join(format!("{}.mp4", super::decoder::PANICS_IN_DEMUX));
    std::fs::copy(fixtures.join("clip.mp4"), &path).unwrap();
    let mut player = Player::open(Source::File(path), None, options(true, false));
    assert!(
        wait_until(Duration::from_secs(5), || player.error().is_some()),
        "no error said"
    );
    assert!(player.is_known(), "it had opened");
    assert!(player.error().unwrap().contains("as the test asked"));
    assert!(!player.is_buffering());
    assert!(!player.tick().animate, "nothing more to draw");
}

/// A paused player with its queues full lets its decode thread sleep,
/// rather than look for something to do hundreds of times a second.
#[test]
fn a_paused_player_lets_its_decoder_sleep() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(fixtures.join("clip.mp4")),
        Some(mixer.clone()),
        options(false, false),
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    // The picture and sound queues fill; then there's nothing to do.
    std::thread::sleep(Duration::from_millis(300));
    let before = player.decoder_waits();
    std::thread::sleep(Duration::from_millis(500));
    let waits = player.decoder_waits() - before;
    assert!(waits <= 5, "it looked {waits} times in half a second");
}

/// What a file holds is shared with whatever asks, every frame, not
/// copied for each; picking a track makes it anew, leaving what was handed
/// out as it was.
#[test]
fn what_a_file_holds_is_shared_not_copied() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut player = Player::open(
        Source::File(fixtures.join("clip-subs.mkv")),
        Some(Mixer::detached(48_000)),
        options(false, false),
    );
    assert!(wait_until(Duration::from_secs(5), || player.is_known()));
    let first = player.info().unwrap();
    assert!(Arc::ptr_eq(&first, &player.info().unwrap()), "copied");
    assert!(player.has_video() && player.has_audio());
    assert_eq!(first.selected(TrackKind::Subtitle), None);

    player.select_track(TrackKind::Subtitle, Some(2));
    let picked = player.info().unwrap();
    assert!(!Arc::ptr_eq(&first, &picked));
    assert_eq!(picked.selected(TrackKind::Subtitle), Some(2));
    assert_eq!(
        first.selected(TrackKind::Subtitle),
        None,
        "what was handed out"
    );
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
    let mut player = Player::open(Source::File(path), None, options(false, false));
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

#[test]
fn reports_the_streams_as_the_file_has_them() {
    let Some(path) = media(
        "details.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
            "-c:v",
            "libx264",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
            "-colorspace",
            "bt709",
            "-c:a",
            "aac",
            "-ac",
            "2",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(Source::File(path), Some(mixer), options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let info = player.info().unwrap();
    assert!(
        info.container.format.contains("MOV"),
        "{:?}",
        info.container
    );
    assert!(info.container.bit_rate.is_some());
    let video = info.video_details.as_ref().unwrap();
    assert_eq!(video.profile.as_deref(), Some("High"));
    assert_eq!(video.coded_size, (320, 240));
    assert_eq!(video.pixel_format, Some("yuv420p"));
    // x264 tags the matrix and range, and leaves the rest unspecified.
    assert_eq!(video.matrix, Some("bt709"));
    assert_eq!(video.full_range, Some(false));
    let audio = info.audio_details.as_ref().unwrap();
    assert_eq!(audio.profile.as_deref(), Some("LC"));
    assert_eq!((audio.sample_rate, audio.channels), (48_000, 2));
    assert_eq!(audio.layout.as_deref(), Some("stereo"));
    assert_eq!(audio.sample_format, Some("fltp"));
    // The decoder reads on past the frame shown, though not always by the
    // time the first one shows.
    let read_on = wait_until(Duration::from_secs(5), || {
        player.tick();
        let stats = player.stats();
        stats.read_ahead > 0.0 && stats.queued_frames > 0
    });
    assert!(read_on, "{:?}", player.stats());
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
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(true, false),
    );
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |_| false);
    assert!(!player.is_playing(), "player should stop at the end");
    // One second of audio, resampled from 44.1 kHz to 48 kHz.
    assert!((heard as i64 - 48_000).abs() < 480, "heard {heard} frames");
    drop(player);
    let _ = Arc::strong_count(&mixer);
}

#[test]
fn plays_on_in_time_with_no_device() {
    let Some(path) = media(
        "half.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=0.5",
            "-ac",
            "2",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    // Plays nowhere until told where: nothing asks it for sound.
    let mixer = output::mixer(1.0);
    let _output = AudioOutput::with_mixer(mixer.clone());
    let mut player = Player::open(Source::File(path), Some(mixer), options(true, false));
    let start = Instant::now();
    while player.is_playing() && start.elapsed() < Duration::from_secs(10) {
        player.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = start.elapsed().as_secs_f64();
    assert!(!player.is_playing(), "the player should get to the end");
    assert!(
        (0.45..3.0).contains(&took),
        "half a second of sound took {took}s"
    );
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
    let mut player = Player::open(Source::File(path), Some(mixer.clone()), options(true, true));
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |heard| {
        heard > 36_000
    });
    assert!(player.is_playing(), "looping player kept playing");
    assert!(
        heard > 36_000,
        "only heard {heard} frames; the file is 12000 long"
    );
}

/// The words of the subtitles showing now.
fn subtitle_text(player: &Player) -> Vec<String> {
    player
        .subtitles()
        .iter()
        .flat_map(|cue| cue.texts.iter().map(|text| text.text.clone()))
        .collect()
}

/// Seek, and wait until the picture is there and the subtitles say `expected`.
fn subtitles_at(player: &mut Player, time: f64, expected: &[&str]) -> Vec<String> {
    player.seek(time);
    let landed = wait_until(Duration::from_secs(5), || {
        player.tick();
        player
            .frame_pts()
            .is_some_and(|pts| (pts - time).abs() < 0.045)
    });
    assert!(landed, "seek to {time} landed on {:?}", player.frame_pts());
    if expected.is_empty() {
        // Nothing showing is also how it starts, so give a wrong cue time
        // to turn up.
        std::thread::sleep(Duration::from_millis(150));
        player.tick();
    } else {
        wait_until(Duration::from_millis(500), || {
            player.tick();
            subtitle_text(player) == expected
        });
    }
    subtitle_text(player)
}

#[test]
fn switches_audio_and_subtitle_tracks() {
    let dir = std::env::temp_dir().join("plays-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.srt");
    let second = dir.join("second.srt");
    std::fs::write(
        &first,
        "1\n00:00:01,000 --> 00:00:02,500\n<i>Hello</i> there\n\n\
         2\n00:00:03,000 --> 00:00:03,800\nSecond line\n",
    )
    .unwrap();
    std::fs::write(&second, "1\n00:00:01,000 --> 00:00:02,500\nBonjour\n").unwrap();
    let Some(path) = media(
        "tracks.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=4",
            "-f",
            "lavfi",
            "-t",
            "4",
            "-i",
            "anullsrc=r=48000:cl=stereo",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=4",
            "-i",
            first.to_str().unwrap(),
            "-i",
            second.to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-map",
            "3",
            "-map",
            "4",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
            // PCM, so the silent track is exactly silent.
            "-c:a",
            "pcm_s16le",
            "-c:s",
            "srt",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "language=ger",
            "-metadata:s:s:0",
            "language=eng",
            "-metadata:s:s:0",
            "title=Full",
            "-metadata:s:s:1",
            "language=fre",
            "-disposition:s:0",
            "0",
            "-disposition:s:1",
            "0",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(false, false),
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));

    let info = player.info().unwrap();
    let audio: Vec<_> = info.tracks(TrackKind::Audio).map(|t| t.index).collect();
    let subtitles: Vec<_> = info.tracks(TrackKind::Subtitle).collect();
    assert_eq!(info.tracks(TrackKind::Video).count(), 1);
    assert_eq!(
        info.tracks(TrackKind::Audio)
            .enumerate()
            .map(|(i, t)| t.label(i + 1))
            .collect::<Vec<_>>(),
        ["English", "German"]
    );
    assert_eq!(subtitles.len(), 2);
    assert_eq!(subtitles[0].label(1), "English – Full");
    assert_eq!(subtitles[0].detail(), "SRT");
    let subtitles: Vec<_> = subtitles.iter().map(|t| t.index).collect();
    assert_eq!(info.audio_track, Some(audio[0]));
    assert_eq!(info.subtitle_track, None, "no subtitle track is a default");

    // The first audio track is silent, the second a tone. Mixing runs
    // faster than real time, so this counts buffers of 512 frames.
    let mix = |player: &mut Player, buffers: usize, enough: usize| {
        let mut heard = 0;
        let mut buffer = vec![0.0f32; 512 * 2];
        for _ in 0..buffers {
            player.tick();
            mixer.mix(&mut buffer, 0.0, Instant::now());
            heard += buffer
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|f| f[0].abs() > 1e-6)
                .count();
            if heard >= enough {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        heard
    };
    player.play();
    assert_eq!(
        mix(&mut player, 40, usize::MAX),
        0,
        "the first track is silent"
    );
    assert!(player.is_playing() && player.position() < 1.0);
    player.select_track(TrackKind::Audio, Some(audio[1]));
    let heard = mix(&mut player, 400, 4800);
    assert!(
        heard >= 4800,
        "heard only {heard} frames of the second track"
    );
    assert_eq!(player.info().unwrap().audio_track, Some(audio[1]));
    player.pause();

    // Picked in the middle of a cue, it shows without a seek, which would
    // drop the frames queued.
    assert_eq!(subtitles_at(&mut player, 1.5, &[]), Vec::<String>::new());
    let serial = player.serial();
    player.select_track(TrackKind::Subtitle, Some(subtitles[0]));
    let shown = wait_until(Duration::from_secs(2), || {
        player.tick();
        subtitle_text(&player) == ["Hello there"]
    });
    assert!(shown, "showing {:?}", subtitle_text(&player));
    assert_eq!(player.serial(), serial, "picking subtitles seeked");
    assert_eq!(
        subtitles_at(&mut player, 1.5, &["Hello there"]),
        ["Hello there"]
    );
    let cues = player.subtitles();
    let spans = &cues[0].texts[0].spans;
    assert!(spans[0].italic && spans[0].len == "Hello".len() && !spans[1].italic);
    assert_eq!(subtitles_at(&mut player, 2.7, &[]), Vec::<String>::new());
    assert_eq!(
        subtitles_at(&mut player, 3.2, &["Second line"]),
        ["Second line"]
    );

    player.select_track(TrackKind::Subtitle, Some(subtitles[1]));
    assert_eq!(subtitles_at(&mut player, 1.2, &["Bonjour"]), ["Bonjour"]);
    assert_eq!(player.info().unwrap().subtitle_track, Some(subtitles[1]));

    player.select_track(TrackKind::Subtitle, None);
    assert!(subtitle_text(&player).is_empty());
    assert_eq!(subtitles_at(&mut player, 1.5, &[]), Vec::<String>::new());
    assert_eq!(player.info().unwrap().subtitle_track, None);
}

/// Seek or skip, and wait until the picture and the playhead are at `expected`.
fn lands_at(player: &mut Player, go: impl FnOnce(&mut Player), expected: f64) -> (f64, f64) {
    go(player);
    wait_until(Duration::from_secs(5), || {
        player.tick();
        !player.is_buffering()
            && player
                .frame_pts()
                .is_some_and(|pts| (pts - expected).abs() < 0.045)
    });
    (player.frame_pts().unwrap_or(f64::NAN), player.position())
}

/// Skips land on the time asked for when a keyframe is near before it, and
/// otherwise on the nearest keyframe their way, which shows at once.
#[test]
fn skips_land_on_keyframes_far_from_the_target() {
    for container in ["mkv", "mp4"] {
        let Some(path) = media(
            &format!("keyframes.{container}"),
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x120:rate=25:duration=12",
                "-c:v",
                "libx264",
                "-x264-params",
                "keyint=100:min-keyint=100:scenecut=0",
                "-pix_fmt",
                "yuv420p",
            ],
        ) else {
            eprintln!("ffmpeg not found, skipping");
            return;
        };
        let mut player = Player::open(Source::File(path), None, options(false, false));
        assert!(wait_until(Duration::from_secs(5), || {
            player.tick();
            player.frame_pts().is_some()
        }));
        let near = |(pts, position): (f64, f64), time: f64| {
            (pts - time).abs() < 0.045 && (position - time).abs() < 0.045
        };

        // Keyframes are at 0, 4 and 8 seconds.
        let landed = lands_at(&mut player, |p| p.seek(1.0), 1.0);
        assert!(
            near(landed, 1.0),
            "{container}: seek to 1 landed at {landed:?}"
        );
        let landed = lands_at(&mut player, |p| p.seek_by(2.0), 4.0);
        assert!(near(landed, 4.0), "{container}: 1+2 landed at {landed:?}");
        let landed = lands_at(&mut player, |p| p.seek_by(0.5), 4.5);
        assert!(near(landed, 4.5), "{container}: 4+0.5 landed at {landed:?}");
        let landed = lands_at(&mut player, |p| p.seek(11.0), 11.0);
        assert!(
            near(landed, 11.0),
            "{container}: seek to 11 landed at {landed:?}"
        );
        let landed = lands_at(&mut player, |p| p.seek_by(-1.5), 8.0);
        assert!(
            near(landed, 8.0),
            "{container}: 11-1.5 landed at {landed:?}"
        );
        let landed = lands_at(&mut player, |p| p.seek_by(-3.5), 4.5);
        assert!(near(landed, 4.5), "{container}: 8-3.5 landed at {landed:?}");
        // A seek is exact however far the keyframe.
        let landed = lands_at(&mut player, |p| p.seek(7.0), 7.0);
        assert!(
            near(landed, 7.0),
            "{container}: seek to 7 landed at {landed:?}"
        );
    }
}

/// Dragging the slider: a scrub queues the first frame it decodes on its
/// way (a keyframe), then the one asked for, and nothing after it, with no
/// sound; one a little ahead decodes on rather than going back to a
/// keyframe. Letting go plays on as before.
#[test]
fn scrubbing_shows_frames_on_the_way_and_nothing_past_them() {
    let Some(path) = media(
        "scrub.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=12",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=12",
            "-c:v",
            "libx264",
            "-x264-params",
            "keyint=100:min-keyint=100:scenecut=0",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(Source::File(path), Some(mixer), options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some() && !player.is_buffering()
    }));
    assert!(player.stats().queued_audio > 0.0, "sound is decoded ahead");
    let near = |time: f64, expected: f64| (time - expected).abs() < 0.045;
    // Not ticked meanwhile, so all it queued is there to see.
    let scrub = |player: &mut Player, time: f64| {
        player.scrub(time);
        assert!(
            wait_until(Duration::from_secs(5), || player
                .queued()
                .last()
                .is_some_and(|&last| near(last, time))),
            "a scrub to {time} queued {:?}",
            player.queued()
        );
        // Nothing more comes.
        std::thread::sleep(Duration::from_millis(100));
        player.queued()
    };

    // Keyframes are at 0, 4 and 8 seconds.
    // Of the frames between, a few at most.
    let queued = scrub(&mut player, 7.0);
    assert!(
        (2..=10).contains(&queued.len()) && near(queued[0], 4.0),
        "{queued:?}"
    );
    assert!(queued.iter().rev().skip(1).all(|&time| time < 7.0));
    player.tick();
    assert!(player.frame_pts().is_some_and(|pts| near(pts, 7.0)));
    assert!(player.queued().is_empty(), "the latest shows");
    assert!(!player.is_buffering());
    assert_eq!(player.stats().queued_audio, 0.0, "a scrub is silent");

    // Past the keyframe at 8, but near: decoded on from 7.
    let queued = scrub(&mut player, 8.6);
    assert!(queued[0] > 7.0 && queued[0] < 8.0, "{queued:?}");
    player.tick();
    // Back: from the keyframe before.
    let queued = scrub(&mut player, 5.0);
    assert!(near(queued[0], 4.0), "{queued:?}");
    player.tick();
    assert!(player.frame_pts().is_some_and(|pts| near(pts, 5.0)));
    // Far ahead: from the keyframe before.
    let queued = scrub(&mut player, 11.5);
    assert!(near(queued[0], 8.0), "{queued:?}");
    player.tick();

    // Let go: it reads ahead again, sound and all.
    let landed = lands_at(&mut player, |p| p.seek(5.0), 5.0);
    assert!(near(landed.0, 5.0) && near(landed.1, 5.0), "{landed:?}");
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.stats().queued_audio > 0.0 && !player.queued().is_empty()
    }));
}

/// Going back keeps the subtitles on screen, even one that started before
/// the keyframe the seek reads from, and one read again shows once.
#[test]
fn going_back_keeps_the_subtitles() {
    let dir = std::env::temp_dir().join("plays-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let srt = dir.join("across.srt");
    std::fs::write(
        &srt,
        "1\n00:00:01,000 --> 00:00:02,000\nBefore\n\n\
         2\n00:00:03,500 --> 00:00:05,000\nAcross\n",
    )
    .unwrap();
    let Some(path) = media(
        "across.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=8",
            "-i",
            srt.to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            // Keyframes at 0 and 4 seconds.
            "-c:v",
            "libx264",
            "-x264-params",
            "keyint=100:min-keyint=100:scenecut=0",
            "-pix_fmt",
            "yuv420p",
            "-c:s",
            "srt",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let track = player
        .info()
        .unwrap()
        .tracks(TrackKind::Subtitle)
        .next()
        .unwrap()
        .index;
    player.select_track(TrackKind::Subtitle, Some(track));

    assert_eq!(subtitles_at(&mut player, 3.8, &["Across"]), ["Across"]);
    // From the keyframe at 4, the cue isn't read again, but it's kept.
    assert_eq!(subtitles_at(&mut player, 4.6, &["Across"]), ["Across"]);
    // A frame back, it stays on screen while the seek goes on.
    player.seek(4.56);
    assert_eq!(subtitle_text(&player), ["Across"], "blinked");
    assert_eq!(subtitles_at(&mut player, 4.56, &["Across"]), ["Across"]);
    // Read again from the keyframe at 0, each shows once.
    assert_eq!(subtitles_at(&mut player, 3.7, &["Across"]), ["Across"]);
    assert_eq!(subtitles_at(&mut player, 1.5, &["Before"]), ["Before"]);
    assert_eq!(subtitles_at(&mut player, 2.5, &[]), Vec::<String>::new());
}

#[test]
fn plays_subtitle_files_beside_the_video() {
    let dir = std::env::temp_dir().join("plays-tests").join("beside");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("clip.en.srt"),
        "1\n00:00:01,000 --> 00:00:02,500\nHello from a file\n\n\
         2\n00:00:03,000 --> 00:00:03,800\nSecond line\n",
    )
    .unwrap();
    // Legacy encodings are common in subtitle files.
    let (russian, _, _) = encoding_rs::WINDOWS_1251
        .encode("1\n00:00:01,000 --> 00:00:02,500\nПривет из файла, как дела?\n");
    std::fs::write(dir.join("clip.ru.srt"), &russian).unwrap();
    let Some(path) = media(
        "beside/clip.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=4",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));

    let info = player.info().unwrap();
    let subtitles: Vec<_> = info.tracks(TrackKind::Subtitle).collect();
    let labels: Vec<_> = subtitles
        .iter()
        .enumerate()
        .map(|(i, t)| (t.label(i + 1), t.detail()))
        .collect();
    assert_eq!(
        labels,
        [
            ("English".to_owned(), "SRT · External".to_owned()),
            ("Russian".to_owned(), "SRT · External".to_owned()),
        ]
    );
    let subtitles: Vec<_> = subtitles.iter().map(|t| t.index).collect();
    assert_eq!(
        info.subtitle_track,
        Some(subtitles[0]),
        "a file beside the video shows from the start"
    );

    assert_eq!(
        subtitles_at(&mut player, 3.2, &["Second line"]),
        ["Second line"]
    );
    // Back, and into a cue that started before the seek.
    assert_eq!(
        subtitles_at(&mut player, 2.0, &["Hello from a file"]),
        ["Hello from a file"]
    );
    assert_eq!(subtitles_at(&mut player, 2.7, &[]), Vec::<String>::new());

    player.select_track(TrackKind::Subtitle, Some(subtitles[1]));
    assert_eq!(
        subtitles_at(&mut player, 1.5, &["Привет из файла, как дела?"]),
        ["Привет из файла, как дела?"]
    );
    assert_eq!(player.info().unwrap().subtitle_track, Some(subtitles[1]));
}

#[test]
fn reports_unplayable_files() {
    let path = std::env::temp_dir().join(format!("plays-not-media-{}.txt", std::process::id()));
    std::fs::write(&path, "definitely not a video").unwrap();
    ffmpeg_next::init().unwrap();
    let mut player = Player::open(Source::File(path.clone()), None, options(true, false));
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

/// A file that says to turn its pictures (as phones do) reports which way up
/// they go, and is sized as decoded.
#[test]
fn reports_the_streams_orientation() {
    let Some(upright) = media(
        "orientation-source.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let upright = upright.to_str().unwrap();
    // A quarter turn counterclockwise, then mirrored, with the pictures as
    // they were.
    let Some(path) = media(
        "orientation.mp4",
        &[
            "-display_rotation",
            "90",
            "-display_hflip",
            "-i",
            upright,
            "-c",
            "copy",
        ],
    ) else {
        panic!("ffmpeg couldn't remux with a display matrix");
    };
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let info = player.info().unwrap();
    assert_eq!(info.video_size, Some((320, 240)));
    assert_eq!(
        info.orientation,
        Orientation::new(-1, false).flipped_horizontally()
    );
}

/// Delaying the sound moves the picture, and the playhead, ahead of it:
/// the sound plays on as it was.
#[test]
fn delays_the_sound_against_the_picture() {
    let Some(path) = media(
        "sync.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=4",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "pcm_s16le",
            "-ac",
            "2",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(false, false),
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    player.set_audio_delay(2.0);
    assert_eq!(player.audio_delay(), 2.0);
    player.play();
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(5), |heard| {
        heard >= 24_000
    });
    let heard = heard as f64 / 48_000.0;
    let ahead = player.position() - heard;
    assert!(
        (ahead - 2.0).abs() < 0.1,
        "the playhead is {ahead:.3} s ahead of the sound"
    );
    assert!(
        wait_until(Duration::from_secs(5), || {
            player.tick();
            player.frame_pts().is_some_and(|pts| pts > heard + 1.8)
        }),
        "the picture is at {:?} after {heard:.2} s of sound",
        player.frame_pts()
    );
    player.set_audio_delay(-60.0);
    assert_eq!(player.audio_delay(), -super::player::MAX_AUDIO_DELAY);
}

/// A subtitle delay moves when cues show, and seeking keeps it: a seek
/// lands on a cue that is over by its own time but not as delayed.
#[test]
fn delays_subtitles_against_the_picture() {
    let dir = std::env::temp_dir().join("plays-tests").join("delay");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("clip.srt"),
        "1\n00:00:01,000 --> 00:00:02,000\nRight on time\n",
    )
    .unwrap();
    let Some(path) = media(
        "delay/clip.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=4",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let cue = ["Right on time"];
    let none = Vec::<String>::new();
    assert_eq!(subtitles_at(&mut player, 1.5, &cue), cue);

    player.set_subtitle_delay(1.0);
    assert_eq!(subtitles_at(&mut player, 1.5, &[]), none);
    assert_eq!(subtitles_at(&mut player, 2.8, &cue), cue);
    assert_eq!(subtitles_at(&mut player, 3.1, &[]), none);

    player.set_subtitle_delay(-0.8);
    assert_eq!(subtitles_at(&mut player, 0.5, &cue), cue);
    assert_eq!(subtitles_at(&mut player, 1.5, &[]), none);
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
    let mut player = Player::open(Source::File(path), None, options(true, false));
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(2500) {
        player.tick();
        std::thread::sleep(Duration::from_millis(200));
    }
    player.tick();
    let behind = player.position() - player.frame_pts().unwrap();
    assert!(behind < 0.3, "video is {behind:.2}s behind the clock");
}

#[test]
fn decodes_on_the_gpu_when_available() {
    let Some(path) = media(
        "video640.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=30:duration=3",
            "-c:v",
            "libx264",
            "-g",
            "60",
            "-pix_fmt",
            "yuv420p",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let mut player = Player::open(
        Source::File(path),
        None,
        PlayerOptions {
            hardware_decoding: true,
            ..options(false, false)
        },
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let info = player.info().unwrap();
    assert_eq!(info.video_size, Some((640, 360)));
    if cfg!(target_os = "macos") {
        assert_eq!(info.hardware, Some("VideoToolbox"));
    } else {
        eprintln!("hardware decoding: {:?}", info.hardware);
    }
    // Seeking flushes the hardware decoder and still lands on the right frame.
    for target in [1.5, 0.2, 2.9] {
        player.seek(target);
        assert!(
            wait_until(Duration::from_secs(5), || {
                player.tick();
                player
                    .frame_pts()
                    .is_some_and(|pts| (pts - target).abs() < 0.04)
            }),
            "seek to {target} landed on {:?}",
            player.frame_pts()
        );
    }
}

/// Hardware-decoded frames stay on the GPU and are drawn by gpui's surface
/// shader, which always converts with the BT.601 matrix from full-range NV12.
/// Whatever the video's own matrix and range, they must come out in the same
/// colors as the same frame converted to BGRA in software.
#[cfg(target_os = "macos")]
#[test]
fn surfaces_match_software_colors() {
    use super::picture::Picture;

    let cases: &[(&str, &str, &[&str])] = &[
        // Untagged video is assumed BT.601 below 720 lines and BT.709 from there.
        ("colors-sd.mp4", "640x360", &["-pix_fmt", "yuv420p"]),
        ("colors-hd.mp4", "1280x720", &["-pix_fmt", "yuv420p"]),
        (
            "colors-709-sd.mp4",
            "640x360",
            &["-pix_fmt", "yuv420p", "-colorspace", "bt709"],
        ),
        (
            "colors-601-hd.mp4",
            "1280x720",
            &["-pix_fmt", "yuv420p", "-colorspace", "smpte170m"],
        ),
        (
            "colors-full.mp4",
            "640x360",
            &["-pix_fmt", "yuvj420p", "-color_range", "pc"],
        ),
    ];
    for &(name, size, extra) in cases {
        let source = format!("testsrc2=size={size}:rate=30:duration=1");
        let mut args = vec!["-f", "lavfi", "-i", &source, "-c:v", "libx264", "-crf", "8"];
        args.extend_from_slice(extra);
        let Some(path) = media(name, &args) else {
            eprintln!("ffmpeg not found, skipping");
            return;
        };
        let first_frame = |hardware| {
            let mut player = Player::open(
                Source::File(path.clone()),
                None,
                PlayerOptions {
                    hardware_decoding: hardware,
                    ..options(false, false)
                },
            );
            assert!(wait_until(Duration::from_secs(5), || {
                player.tick();
                player.frame_pts().is_some()
            }));
            player.frame().unwrap()
        };
        let Picture::Surface(surface) = first_frame(true) else {
            panic!("{name}: hardware frames should stay on the GPU");
        };
        let Picture::Image(image) = first_frame(false) else {
            panic!("{name}: software frames should be images");
        };

        let surface = surface_to_rgb(surface.buffer());
        let (width, height) = (surface.width, surface.height);
        let bgra = image.as_bytes(0).unwrap();
        assert_eq!(bgra.len(), width * height * 4, "{name}: sizes differ");
        let software = |x: usize, y: usize, c: usize| f64::from(bgra[(y * width + x) * 4 + 2 - c]);

        // Compare 8x8 block averages. VideoToolbox resamples chroma a little
        // differently from swscale while converting, which shows along sharp
        // color edges (the timecode, the thin rainbow line), so judge the bulk
        // of the picture. Done right, 90% of blocks are within about a level;
        // a wrong matrix puts that at 20 levels or more.
        let mut diffs = Vec::new();
        for by in (0..height).step_by(8) {
            for bx in (0..width).step_by(8) {
                let (ys, xs) = (by..(by + 8).min(height), bx..(bx + 8).min(width));
                let n = (ys.len() * xs.len()) as f64;
                for c in 0..3 {
                    let (mut a, mut b) = (0.0, 0.0);
                    for y in ys.clone() {
                        for x in xs.clone() {
                            a += surface.get(x, y, c);
                            b += software(x, y, c);
                        }
                    }
                    diffs.push((a - b).abs() / n);
                }
            }
        }
        diffs.sort_by(f64::total_cmp);
        let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
        let p90 = diffs[diffs.len() * 9 / 10];
        assert!(
            mean < 1.5 && p90 < 3.0,
            "{name}: colors differ by {mean:.2} levels on average, {p90:.2} at the 90th percentile"
        );
    }
}

/// RGB (0-255) of a surface as gpui's Metal shader computes it.
#[cfg(target_os = "macos")]
struct SurfaceRgb {
    width: usize,
    height: usize,
    rgb: Vec<f64>,
}

#[cfg(target_os = "macos")]
impl SurfaceRgb {
    fn get(&self, x: usize, y: usize, c: usize) -> f64 {
        self.rgb[(y * self.width + x) * 3 + c]
    }
}

#[cfg(target_os = "macos")]
fn surface_to_rgb(buffer: &core_video::pixel_buffer::CVPixelBuffer) -> SurfaceRgb {
    use core_video::pixel_buffer::{
        kCVPixelBufferLock_ReadOnly, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    };

    assert_eq!(
        buffer.get_pixel_format(),
        kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
    );
    let (width, height) = (buffer.get_width(), buffer.get_height());
    assert_eq!(buffer.lock_base_address(kCVPixelBufferLock_ReadOnly), 0);
    let plane = |index: usize| {
        let rows = buffer.get_height_of_plane(index);
        let stride = buffer.get_bytes_per_row_of_plane(index);
        // SAFETY: the buffer is locked, and a plane is `rows * stride` bytes.
        unsafe {
            std::slice::from_raw_parts(
                buffer.get_base_address_of_plane(index) as *const u8,
                rows * stride,
            )
        }
    };
    let (luma, chroma) = (plane(0), plane(1));
    let luma_stride = buffer.get_bytes_per_row_of_plane(0);
    let chroma_stride = buffer.get_bytes_per_row_of_plane(1);
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let l = f64::from(luma[y * luma_stride + x]) / 255.0;
            let i = (y / 2) * chroma_stride + (x / 2) * 2;
            let cb = f64::from(chroma[i]) / 255.0;
            let cr = f64::from(chroma[i + 1]) / 255.0;
            // gpui's `surface_fragment` matrix.
            let r = l + 1.402 * cr - 0.701;
            let g = l - 0.3441 * cb - 0.7141 * cr + 0.5291;
            let b = l + 1.772 * cb - 0.886;
            rgb.extend([r, g, b].map(|v| v.clamp(0.0, 1.0) * 255.0));
        }
    }
    buffer.unlock_base_address(kCVPixelBufferLock_ReadOnly);
    SurfaceRgb { width, height, rgb }
}

/// HDR video on the GPU becomes 10-bit BT.2020 surfaces tagged with their
/// transfer function, which Plays' gpui draws with its HDR shader. For an SDR
/// screen that shader must give the same picture as the CPU's tone mapping of
/// the same frame decoded in software.
#[cfg(target_os = "macos")]
#[test]
fn hdr_surfaces_match_software_tone_mapping() {
    use core_foundation::base::TCFType;
    use core_video::buffer::CVBufferGetAttachment;
    use core_video::image_buffer::{
        CVTransferFunctionGetIntegerCodePointForString, kCVImageBufferTransferFunctionKey,
    };
    use core_video::pixel_buffer::kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange;

    use super::hdr::Transfer;
    use super::picture::Picture;

    let cases = [
        ("hdr10.mp4", "smpte2084", Transfer::Pq, 16),
        ("hlg.mp4", "arib-std-b67", Transfer::Hlg, 18),
    ];
    for (name, trc, transfer, code_point) in cases {
        let x265 = format!("log-level=error:colorprim=bt2020:transfer={trc}:colormatrix=bt2020nc");
        let args = [
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=30:duration=1",
            "-c:v",
            "libx265",
            "-tag:v",
            "hvc1",
            "-crf",
            "8",
            "-pix_fmt",
            "yuv420p10le",
            "-color_primaries",
            "bt2020",
            "-color_trc",
            trc,
            "-colorspace",
            "bt2020nc",
            "-x265-params",
            &x265,
        ];
        let Some(path) = media(name, &args) else {
            eprintln!("ffmpeg with libx265 not found, skipping");
            return;
        };
        let first_frame = |hardware| {
            let mut player = Player::open(
                Source::File(path.clone()),
                None,
                PlayerOptions {
                    hardware_decoding: hardware,
                    ..options(false, false)
                },
            );
            assert!(wait_until(Duration::from_secs(5), || {
                player.tick();
                player.frame_pts().is_some()
            }));
            player.frame().unwrap()
        };
        let Picture::Surface(surface) = first_frame(true) else {
            panic!("{name}: hardware frames should stay on the GPU");
        };
        let Picture::Image(image) = first_frame(false) else {
            panic!("{name}: software frames should be images");
        };

        let buffer = surface.buffer();
        assert_eq!(
            buffer.get_pixel_format(),
            kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
            "{name}: HDR surfaces should be 10-bit"
        );
        // SAFETY: reading a tag of a valid buffer.
        let tagged = unsafe {
            let value = CVBufferGetAttachment(
                buffer.as_concrete_TypeRef(),
                kCVImageBufferTransferFunctionKey,
                std::ptr::null_mut(),
            );
            (!value.is_null()).then(|| CVTransferFunctionGetIntegerCodePointForString(value.cast()))
        };
        assert_eq!(tagged, Some(code_point), "{name}: transfer function tag");

        let surface = hdr_surface_to_sdr(buffer, transfer);
        let (width, height) = (surface.width, surface.height);
        let bgra = image.as_bytes(0).unwrap();
        assert_eq!(bgra.len(), width * height * 4, "{name}: sizes differ");
        let software = |x: usize, y: usize, c: usize| f64::from(bgra[(y * width + x) * 4 + 2 - c]);

        // As in `surfaces_match_software_colors`, compare 8x8 block averages.
        let mut diffs = Vec::new();
        for by in (0..height).step_by(8) {
            for bx in (0..width).step_by(8) {
                let (ys, xs) = (by..(by + 8).min(height), bx..(bx + 8).min(width));
                let n = (ys.len() * xs.len()) as f64;
                for c in 0..3 {
                    let (mut a, mut b) = (0.0, 0.0);
                    for y in ys.clone() {
                        for x in xs.clone() {
                            a += surface.get(x, y, c);
                            b += software(x, y, c);
                        }
                    }
                    diffs.push((a - b).abs() / n);
                }
            }
        }
        diffs.sort_by(f64::total_cmp);
        let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
        let p90 = diffs[diffs.len() * 9 / 10];
        assert!(
            mean < 1.5 && p90 < 3.0,
            "{name}: colors differ by {mean:.2} levels on average, {p90:.2} at the 90th percentile"
        );
    }
}

/// RGB (0-255) of a 10-bit HDR surface as gpui's `surface_hdr_fragment`
/// computes it for an SDR screen (EDR headroom 1).
#[cfg(target_os = "macos")]
fn hdr_surface_to_sdr(
    buffer: &core_video::pixel_buffer::CVPixelBuffer,
    transfer: super::hdr::Transfer,
) -> SurfaceRgb {
    use core_video::pixel_buffer::kCVPixelBufferLock_ReadOnly;

    use super::hdr::{
        REFERENCE_WHITE_NITS, Transfer, bt2020_to_bt709, hlg_scene_to_nits, hlg_to_scene,
        pq_to_nits, srgb_encode, tone_map,
    };

    let (width, height) = (buffer.get_width(), buffer.get_height());
    assert_eq!(buffer.lock_base_address(kCVPixelBufferLock_ReadOnly), 0);
    let plane = |index: usize| {
        let rows = buffer.get_height_of_plane(index);
        let stride = buffer.get_bytes_per_row_of_plane(index);
        // SAFETY: the buffer is locked, and a plane is `rows * stride` bytes
        // of 16-bit samples.
        unsafe {
            std::slice::from_raw_parts(
                buffer.get_base_address_of_plane(index) as *const u16,
                rows * stride / 2,
            )
        }
    };
    let (luma, chroma) = (plane(0), plane(1));
    let luma_stride = buffer.get_bytes_per_row_of_plane(0) / 2;
    let chroma_stride = buffer.get_bytes_per_row_of_plane(1) / 2;
    // BT.2020, video range; the 10 bits are the top ones of each sample.
    let (kr, kb) = (0.2627f32, 0.0593f32);
    let kg = 1.0 - kr - kb;
    let code = |sample: u16| f32::from(sample >> 6);
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let l = (code(luma[y * luma_stride + x]) - 64.0) / 876.0;
            let i = (y / 2) * chroma_stride + (x / 2) * 2;
            let cb = (code(chroma[i]) - 512.0) / 896.0;
            let cr = (code(chroma[i + 1]) - 512.0) / 896.0;
            let signal = [
                l + 2.0 * (1.0 - kr) * cr,
                l - 2.0 * kb * (1.0 - kb) / kg * cb - 2.0 * kr * (1.0 - kr) / kg * cr,
                l + 2.0 * (1.0 - kb) * cb,
            ]
            .map(|v| v.clamp(0.0, 1.0));
            let light = match transfer {
                Transfer::Pq => signal.map(|s| pq_to_nits(s) / REFERENCE_WHITE_NITS),
                Transfer::Hlg => hlg_scene_to_nits(signal.map(hlg_to_scene))
                    .map(|nits| nits / REFERENCE_WHITE_NITS),
            };
            let mapped = tone_map(bt2020_to_bt709(light), 1.0);
            rgb.extend(mapped.map(|v| f64::from(srgb_encode(v.clamp(0.0, 1.0))) * 255.0));
        }
    }
    buffer.unlock_base_address(kCVPixelBufferLock_ReadOnly);
    SurfaceRgb { width, height, rgb }
}

/// Dolby Vision profile 5 can't be made with the ffmpeg tool, so this runs
/// on a file of your own: `PLAYS_DOLBY_VISION_SAMPLE=path cargo test dolby`.
/// Its surfaces must carry the frame's reshaping and, through the Dolby
/// Vision shader's math, show what software decoding shows. Both pictures
/// are saved as PNGs next to the generated test media.
#[cfg(target_os = "macos")]
#[test]
fn dolby_vision_surfaces_match_software() {
    use core_foundation::base::TCFType;
    use core_foundation::data::{CFData, CFDataRef};
    use core_foundation::string::CFString;
    use core_video::buffer::CVBufferGetAttachment;
    use core_video::pixel_buffer::kCVPixelBufferLock_ReadOnly;

    use super::dovi::ShaderParams;
    use super::hdr::{REFERENCE_WHITE_NITS, bt2020_to_bt709, srgb_encode, tone_map};
    use super::picture::Picture;
    use super::videotoolbox::DOLBY_VISION_ATTACHMENT;

    let Some(path) = std::env::var_os("PLAYS_DOLBY_VISION_SAMPLE").map(PathBuf::from) else {
        eprintln!("PLAYS_DOLBY_VISION_SAMPLE isn't set, skipping");
        return;
    };
    ffmpeg_next::init().unwrap();
    let out_dir = std::env::temp_dir().join("plays-tests");
    std::fs::create_dir_all(&out_dir).unwrap();
    for share in [0.2, 0.5] {
        let frame_at = |hardware| {
            let mut player = Player::open(
                Source::File(path.clone()),
                None,
                PlayerOptions {
                    hardware_decoding: hardware,
                    max_short_side: Some(720),
                    ..options(false, false)
                },
            );
            assert!(wait_until(Duration::from_secs(10), || {
                player.tick();
                player.duration().is_some()
            }));
            let time = (player.duration().unwrap() * share).round();
            player.seek(time);
            assert!(wait_until(Duration::from_secs(30), || {
                player.tick();
                player.frame_pts().is_some_and(|pts| pts >= time - 1.0)
            }));
            (player.frame().unwrap(), time)
        };
        let (Picture::Surface(surface), time) = frame_at(true) else {
            panic!("hardware frames should stay on the GPU");
        };
        let (Picture::Image(image), _) = frame_at(false) else {
            panic!("software frames should be images");
        };

        let buffer = surface.buffer();
        let key = CFString::from_static_string(DOLBY_VISION_ATTACHMENT);
        // SAFETY: reading an attachment of a valid buffer.
        let params = unsafe {
            let value = CVBufferGetAttachment(
                buffer.as_concrete_TypeRef(),
                key.as_concrete_TypeRef(),
                std::ptr::null_mut(),
            );
            assert!(!value.is_null(), "surfaces should carry the reshaping");
            ShaderParams::from_bytes(CFData::wrap_under_get_rule(value as CFDataRef).bytes())
        };

        // `surface_dovi_fragment`, for an SDR screen.
        let (width, height) = (buffer.get_width(), buffer.get_height());
        assert_eq!(buffer.lock_base_address(kCVPixelBufferLock_ReadOnly), 0);
        let plane = |index: usize| {
            let rows = buffer.get_height_of_plane(index);
            let stride = buffer.get_bytes_per_row_of_plane(index) / 2;
            // SAFETY: the buffer is locked, and a plane is `rows` rows of
            // `stride` 16-bit samples.
            let samples = unsafe {
                std::slice::from_raw_parts(
                    buffer.get_base_address_of_plane(index) as *const u16,
                    rows * stride,
                )
            };
            (samples, stride)
        };
        let ((luma, luma_stride), (chroma, chroma_stride)) = (plane(0), plane(1));
        let mut gpu = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                let i = (y / 2) * chroma_stride + (x / 2) * 2;
                let samples = [luma[y * luma_stride + x], chroma[i], chroma[i + 1]]
                    .map(|s| f32::from(s) / 65536.0);
                let light = params
                    .light(samples)
                    .map(|nits| nits / REFERENCE_WHITE_NITS);
                let rgb = tone_map(bt2020_to_bt709(light), 1.0);
                gpu.extend(rgb.map(|v| srgb_encode(v.clamp(0.0, 1.0)) * 255.0));
            }
        }
        buffer.unlock_base_address(kCVPixelBufferLock_ReadOnly);

        let bgra = image.as_bytes(0).unwrap();
        assert_eq!(bgra.len(), width * height * 4, "sizes differ");
        let to_png = |name: &str, rgb: &dyn Fn(usize) -> [u8; 3]| {
            let pixels = (0..width * height).flat_map(rgb).collect();
            let png = out_dir.join(format!("dolby-vision-{time}-{name}.png"));
            image::RgbImage::from_raw(width as u32, height as u32, pixels)
                .unwrap()
                .save(&png)
                .unwrap();
            eprintln!("{}", png.display());
        };
        to_png("gpu", &|i| {
            std::array::from_fn(|c| gpu[i * 3 + c].round() as u8)
        });
        to_png("cpu", &|i| [bgra[i * 4 + 2], bgra[i * 4 + 1], bgra[i * 4]]);

        let mut diffs = Vec::new();
        for by in (0..height).step_by(8) {
            for bx in (0..width).step_by(8) {
                let (ys, xs) = (by..(by + 8).min(height), bx..(bx + 8).min(width));
                let n = (ys.len() * xs.len()) as f64;
                for c in 0..3 {
                    let (mut a, mut b) = (0.0, 0.0);
                    for y in ys.clone() {
                        for x in xs.clone() {
                            a += f64::from(gpu[(y * width + x) * 3 + c]);
                            b += f64::from(bgra[(y * width + x) * 4 + 2 - c]);
                        }
                    }
                    diffs.push((a - b).abs() / n);
                }
            }
        }
        diffs.sort_by(f64::total_cmp);
        let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
        let p90 = diffs[diffs.len() * 9 / 10];
        eprintln!("{time}s: {mean:.2} levels apart on average, {p90:.2} at the 90th percentile");
        assert!(
            mean < 1.5 && p90 < 3.0,
            "{time}s: colors differ by {mean:.2} levels on average, {p90:.2} at the 90th percentile"
        );
    }
}

// ---------------------------------------------------------------------------
// Network streams, served from a tiny HTTP server on localhost.

mod http {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::time::Duration;

    /// Serve files from `dir`, with byte ranges like a real web server. With
    /// `stall`, ranges aren't offered and each response pauses for that long
    /// halfway through, like a congested connection.
    pub fn serve(dir: PathBuf, stall: Option<Duration>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let dir = dir.clone();
                std::thread::spawn(move || respond(stream, &dir, stall));
            }
        });
        addr
    }

    fn respond(mut stream: TcpStream, dir: &std::path::Path, stall: Option<Duration>) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request = String::new();
        if reader.read_line(&mut request).is_err() {
            return;
        }
        let mut start = 0;
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
            if let Some(range) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                start = range
                    .split('-')
                    .next()
                    .and_then(|n| n.trim().parse().ok())
                    .unwrap_or(0);
            }
            line.clear();
        }
        let path = request.split_whitespace().nth(1).unwrap_or("/");
        let Ok(body) = std::fs::read(dir.join(path.trim_start_matches('/'))) else {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        };
        let len = body.len();
        let header = if stall.is_some() {
            format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n")
        } else if start > 0 {
            if start >= len {
                let _ = stream.write_all(format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
                return;
            }
            format!(
                "HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                len - 1,
                len - start
            )
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
            )
        };
        let body = if stall.is_some() {
            &body[..]
        } else {
            &body[start..]
        };
        let _ = stream.write_all(header.as_bytes());
        let (first, rest) = body.split_at(if stall.is_some() {
            body.len() / 2
        } else {
            body.len()
        });
        let _ = stream.write_all(first);
        if let Some(stall) = stall {
            let _ = stream.flush();
            std::thread::sleep(stall);
        }
        let _ = stream.write_all(rest);
    }
}

/// A short video with sound, as MPEG-TS, plus an HLS version of it in `hls/`.
fn stream_media() -> Option<PathBuf> {
    let ts = media(
        "stream.ts",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=4",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-ac",
            "2",
        ],
    )?;
    let dir = ts.parent()?.to_path_buf();
    let hls = dir.join("hls");
    if !hls.join("vod.m3u8").exists() {
        std::fs::create_dir_all(&hls).ok()?;
        let status = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&ts)
            .args([
                "-c",
                "copy",
                "-f",
                "hls",
                "-hls_time",
                "1",
                "-hls_list_size",
                "0",
            ])
            .args(["-hls_playlist_type", "vod", "-hls_segment_filename"])
            .arg(hls.join("segment%d.ts"))
            .arg(hls.join("vod.m3u8"))
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
        // The same segments without an end: what a live stream looks like.
        let vod = std::fs::read_to_string(hls.join("vod.m3u8")).ok()?;
        let live: String = vod
            .lines()
            .filter(|l| !l.starts_with("#EXT-X-ENDLIST") && !l.starts_with("#EXT-X-PLAYLIST-TYPE"))
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(hls.join("live.m3u8"), live).ok()?;
    }
    Some(dir)
}

#[test]
fn plays_a_file_over_http() {
    let Some(dir) = stream_media() else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let addr = http::serve(dir, None);
    let mixer = Mixer::detached(48_000);
    let source = Source::parse_url(&format!("http://{addr}/stream.ts")).unwrap();
    let mut player = Player::open(source, Some(mixer.clone()), options(true, false));
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(20), |_| false);
    assert!(player.error().is_none(), "{:?}", player.error());
    let info = player.info().unwrap();
    assert!(info.has_video && info.has_audio && !info.live, "{info:?}");
    assert!((info.duration.unwrap() - 4.0).abs() < 0.2, "{info:?}");
    assert!(player.frame_pts().is_some());
    assert!(!player.is_playing(), "the stream should have ended");
    // Four seconds of audio (AAC priming may trim a frame).
    assert!(
        (heard as i64 - 4 * 48_000).abs() < 4_800,
        "heard {heard} frames"
    );
}

#[test]
fn plays_and_seeks_hls() {
    let Some(dir) = stream_media() else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let addr = http::serve(dir, None);
    let source = Source::parse_url(&format!("http://{addr}/hls/vod.m3u8")).unwrap();
    let mut player = Player::open(source, None, options(false, false));
    assert!(
        wait_until(Duration::from_secs(10), || {
            player.tick();
            player.frame_pts().is_some()
        }),
        "{:?}",
        player.error()
    );
    let info = player.info().unwrap();
    assert!(!info.live);
    assert!((info.duration.unwrap() - 4.0).abs() < 0.2, "{info:?}");
    player.seek(2.5);
    assert!(
        wait_until(Duration::from_secs(10), || {
            player.tick();
            player
                .frame_pts()
                .is_some_and(|pts| (pts - 2.5).abs() < 0.045)
        }),
        "seek landed on {:?}",
        player.frame_pts()
    );
}

#[test]
fn treats_playlists_without_an_end_as_live() {
    let Some(dir) = stream_media() else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let addr = http::serve(dir, None);
    let source = Source::parse_url(&format!("http://{addr}/hls/live.m3u8")).unwrap();
    let mut player = Player::open(source, None, options(true, false));
    assert!(
        wait_until(Duration::from_secs(10), || {
            player.tick();
            player.frame_pts().is_some()
        }),
        "{:?}",
        player.error()
    );
    assert!(player.is_live());
    assert_eq!(player.duration(), None);
    // Live streams can't seek; the request is ignored.
    let before = player.position();
    player.seek(before + 30.0);
    assert!(player.position() < before + 1.0);
    // Closing must not wait for the playlist to be reloaded.
    let start = Instant::now();
    drop(player);
    assert!(start.elapsed() < Duration::from_millis(100));
}

#[test]
fn waits_for_a_stalled_stream_and_resumes() {
    let Some(dir) = stream_media() else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let addr = http::serve(dir, Some(Duration::from_secs(3)));
    let mixer = Mixer::detached(48_000);
    let source = Source::parse_url(&format!("http://{addr}/stream.ts")).unwrap();
    let mut player = Player::open(source, Some(mixer.clone()), options(true, false));
    let mut heard = 0;
    let mut stalled = false;
    let mut buffer = vec![0.0f32; 512 * 2];
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        player.tick();
        if !player.is_playing() {
            break;
        }
        stalled |= heard > 0 && player.is_buffering();
        mixer.mix(&mut buffer, 0.0, Instant::now());
        heard += buffer
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|f| f[0].abs() > 1e-6)
            .count();
        std::thread::sleep(Duration::from_micros(300));
    }
    assert!(stalled, "the player never noticed the stall");
    assert!(!player.is_playing(), "the stream should have ended");
    // Everything was heard despite the pause in the middle: the player held
    // the clock instead of skipping ahead.
    assert!(
        (heard as i64 - 4 * 48_000).abs() < 4_800,
        "heard {heard} frames"
    );
}

#[test]
fn reports_unreachable_streams() {
    ffmpeg_next::init().unwrap();
    // Nothing listens on port 9 (discard) on test machines.
    let source = Source::parse_url("http://127.0.0.1:9/missing.ts").unwrap();
    let mut player = Player::open(source, None, options(true, false));
    assert!(wait_until(Duration::from_secs(10), || {
        player.tick();
        player.error().is_some()
    }));
    let error = player.error().unwrap();
    assert!(
        error.contains("could not open http://127.0.0.1:9/missing.ts"),
        "{error}"
    );
}

/// A server that accepts the connection and then never answers. Closing the
/// player must abort the blocked read instead of leaving threads behind.
#[test]
fn closing_aborts_a_hanging_connection() {
    use std::io::Read;
    use std::net::TcpListener;

    ffmpeg_next::init().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let source = Source::parse_url(&format!("http://{addr}/hang.ts")).unwrap();
    let player = Player::open(source, None, options(true, false));
    let (mut connection, _) = listener.accept().unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    // Read the request; FFmpeg now waits for a response that never comes.
    let mut request = [0u8; 4096];
    assert!(connection.read(&mut request).unwrap() > 0);

    drop(player);
    let start = Instant::now();
    // The connection is closed (read returns 0) once FFmpeg gives up.
    let closed = loop {
        match connection.read(&mut request) {
            Ok(0) => break true,
            Ok(_) => continue,
            Err(_) => break false,
        }
    };
    assert!(closed, "the connection was left open");
    assert!(start.elapsed() < Duration::from_secs(2));
}

/// Screenshots decode the frame shown again, at full size whatever the
/// resolution limit, and find exactly that frame though it isn't a keyframe.
#[test]
fn stills_are_the_frame_shown_at_full_size() {
    use super::still;

    // Each frame a flat grey a few levels brighter than the one before.
    let Some(path) = media(
        "frames.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "color=black:size=640x360:rate=25:duration=2,format=yuv420p,\
             geq=lum='16+4*N':cb=128:cr=128",
            "-c:v",
            "libx264",
            "-g",
            "50",
            "-crf",
            "1",
            "-pix_fmt",
            "yuv420p",
        ],
    ) else {
        eprintln!("ffmpeg not found, skipping");
        return;
    };
    let source = Source::File(path);
    let mut player = Player::open(
        source.clone(),
        None,
        PlayerOptions {
            max_short_side: Some(180),
            ..options(false, false)
        },
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    player.seek(1.0);
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player
            .frame_pts()
            .is_some_and(|pts| (pts - 1.0).abs() < 0.01)
    }));
    assert_eq!(player.frame().unwrap().size(), (320, 180));
    let stream = player.info().unwrap().video_track.unwrap();
    let timestamp = player.frame_timestamp().unwrap();

    let grey = |still: &still::Still| f64::from(still.pixels[still.pixels.len() / 2][1]) / 257.0;
    // Frame `n` in full-range RGB.
    let frame = |n: f64| 4.0 * n * 255.0 / 219.0;
    let shot = still::grab(&source, stream, timestamp, false).unwrap();
    assert_eq!((shot.width, shot.height, shot.hdr), (640, 360, None));
    assert!((grey(&shot) - frame(25.0)).abs() < 1.5, "{}", grey(&shot));
    // The first frame, which is a keyframe, too.
    let first = still::grab(&source, stream, 0, false).unwrap();
    assert!(grey(&first) < 1.5, "{}", grey(&first));
}

/// HDR stills either look as an SDR screen shows them or keep their PQ
/// signal, which tone maps to the same picture.
#[test]
fn hdr_stills_keep_their_signal() {
    use super::hdr::{REFERENCE_WHITE_NITS, ToneMapper, Transfer, pq_to_nits};
    use super::still;

    let Some(path) = media(
        "still-pq.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=25:duration=1",
            "-c:v",
            "libx265",
            "-crf",
            "8",
            "-pix_fmt",
            "yuv420p10le",
            "-color_primaries",
            "bt2020",
            "-color_trc",
            "smpte2084",
            "-colorspace",
            "bt2020nc",
            "-x265-params",
            "log-level=error:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc",
        ],
    ) else {
        eprintln!("ffmpeg with libx265 not found, skipping");
        return;
    };
    let source = Source::File(path);
    let sdr = still::grab(&source, 0, 0, false).unwrap();
    let hdr = still::grab(&source, 0, 0, true).unwrap();
    assert_eq!(sdr.hdr, None);
    assert_eq!(hdr.hdr, Some(Transfer::Pq));
    assert_eq!((hdr.width, hdr.height), (sdr.width, sdr.height));
    let mapper = ToneMapper::new(Transfer::Pq);
    let mut worst = 0;
    for (kept, shown) in hdr.pixels.iter().zip(&sdr.pixels).step_by(97) {
        let light = kept.map(|s| pq_to_nits(f32::from(s) / 65535.0) / REFERENCE_WHITE_NITS);
        let [b, g, r, _] = mapper.output(light);
        for (mapped, shown) in [r, g, b].into_iter().zip(shown) {
            worst = worst.max((i32::from(mapped) - i32::from(shown / 257)).abs());
        }
    }
    assert!(worst <= 3, "kept HDR tone maps {worst} levels off");
}
