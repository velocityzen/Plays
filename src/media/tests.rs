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
use super::source::Source;

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
