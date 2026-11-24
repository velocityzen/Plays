//! HLS master playlists over HTTP: the variant picked plays alone.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::http::{self, Asked};
use super::made::{kept, made_in};
use super::{options, wait_until};
use crate::media::decoder::fetch::fetch;
use crate::media::player::{Player, PlayerOptions};
use crate::media::source::Source;
use crate::media::still;

/// The variants, by folder, and the size of each.
const VARIANTS: [(&str, &str); 2] = [("big", "320x240"), ("small", "160x120")];

/// A master playlist listing both, the bigger last.
const MASTER: &str = "#EXTM3U\n\
    #EXT-X-STREAM-INF:BANDWIDTH=300000,RESOLUTION=160x120\n\
    small/index.m3u8\n\
    #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=320x240\n\
    big/index.m3u8\n";

/// The same, its bigger variant gone.
const GONE: &str = "#EXTM3U\n\
    #EXT-X-STREAM-INF:BANDWIDTH=300000,RESOLUTION=160x120\n\
    small/index.m3u8\n\
    #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=320x240\n\
    gone/index.m3u8\n";

/// A short video with sound as HLS in two sizes, each in its folder, and
/// `master.m3u8` and `gone.m3u8` listing them.
fn variants() -> PathBuf {
    made_in(
        &kept(),
        "variants",
        &format!("{VARIANTS:?} {MASTER} {GONE}"),
        |dir| {
            std::fs::create_dir(dir).map_err(|err| err.to_string())?;
            for (name, size) in VARIANTS {
                make_variant(&dir.join(name), size)?;
            }
            std::fs::write(dir.join("master.m3u8"), MASTER).map_err(|err| err.to_string())?;
            std::fs::write(dir.join("gone.m3u8"), GONE).map_err(|err| err.to_string())
        },
    )
}

/// Four seconds of video `size`, with sound, as HLS in `folder`.
fn make_variant(folder: &Path, size: &str) -> Result<(), String> {
    std::fs::create_dir(folder).map_err(|err| err.to_string())?;
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg(format!("testsrc=size={size}:rate=25:duration=4"))
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=4",
        ])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(["-c:a", "aac", "-ac", "2", "-f", "hls", "-hls_time", "1"])
        .args(["-hls_list_size", "0", "-hls_playlist_type", "vod"])
        .arg("-hls_segment_filename")
        .arg(folder.join("segment%d.ts"))
        .arg(folder.join("index.m3u8"))
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg couldn't make the {size} variant"))
    }
}

/// `url` opened with `options`, once its first frame shows.
fn showing(url: &str, options: PlayerOptions) -> Player {
    let mut player = Player::open(Source::parse_url(url).unwrap(), None, options);
    assert!(
        wait_until(Duration::from_secs(20), || {
            player.tick();
            player.frame_pts().is_some()
        }),
        "{:?}",
        player.error()
    );
    player
}

/// The size of the picture `player` shows.
fn size(player: &Player) -> Option<(u32, u32)> {
    player.info().unwrap().video_size
}

/// Whether a path in `folder` was asked for.
fn asked_in(asked: &Asked, folder: &str) -> bool {
    let prefix = format!("/{folder}/");
    asked
        .lock()
        .unwrap()
        .iter()
        .any(|path| path.starts_with(&prefix))
}

/// Answer every request at a new address with a redirect to `to`'s.
fn redirecting(to: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{to}/master.m3u8\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    addr
}

#[test]
fn a_master_playlist_plays_its_best_variant_alone() {
    let (addr, asked) = http::serve_noting(variants(), None);
    let player = showing(&format!("http://{addr}/master.m3u8"), options(false, false));
    assert_eq!(size(&player), Some((320, 240)));
    assert!(asked_in(&asked, "big"));
    assert!(!asked_in(&asked, "small"), "{:?}", asked.lock().unwrap());
}

#[test]
fn a_master_playlist_plays_the_variant_within_the_resolution_limit() {
    let (addr, asked) = http::serve_noting(variants(), None);
    let limited = PlayerOptions {
        max_short_side: Some(200),
        ..options(false, false)
    };
    let player = showing(&format!("http://{addr}/master.m3u8"), limited);
    assert_eq!(size(&player), Some((160, 120)));
    assert!(!asked_in(&asked, "big"), "{:?}", asked.lock().unwrap());

    // A screenshot of it is of the same variant.
    assert_eq!(player.max_short_side(), Some(200));
    let video = player.info().unwrap().video_track.unwrap();
    let still = still::grab(player.source(), player.max_short_side(), video, 0, false).unwrap();
    assert_eq!((still.width, still.height), (160, 120));
    assert!(!asked_in(&asked, "big"), "{:?}", asked.lock().unwrap());
}

#[test]
fn a_master_playlist_reached_by_a_redirect_plays_its_variant_from_where_it_is() {
    let (addr, asked) = http::serve_noting(variants(), None);
    let moved = redirecting(addr);
    let player = showing(
        &format!("http://{moved}/master.m3u8"),
        options(false, false),
    );
    assert_eq!(size(&player), Some((320, 240)));
    assert!(!asked_in(&asked, "small"), "{:?}", asked.lock().unwrap());
}

#[test]
fn a_master_playlist_plays_whole_when_its_variant_alone_wont_open() {
    let (addr, asked) = http::serve_noting(variants(), None);
    let player = showing(&format!("http://{addr}/gone.m3u8"), options(false, false));
    assert_eq!(size(&player), Some((160, 120)));
    assert!(asked_in(&asked, "gone"));
}

#[test]
fn fetches_a_file_whole_and_says_where_it_came_from() {
    ffmpeg_next::init().unwrap();
    let dir = variants();
    let addr = http::serve(dir.clone(), None);
    let moved = redirecting(addr);
    let fetched = fetch(
        &format!("http://{moved}/old.m3u8"),
        ffmpeg_next::Dictionary::new(),
        MASTER.len(),
        || false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(fetched.body, MASTER.as_bytes());
    assert_eq!(fetched.location, format!("http://{addr}/master.m3u8"));

    // Unmoved, it came from where it was asked for.
    let url = format!("http://{addr}/master.m3u8");
    let fetched = fetch(&url, ffmpeg_next::Dictionary::new(), MASTER.len(), || false);
    assert_eq!(fetched.unwrap().unwrap().location, url);
}

#[test]
fn fetches_nothing_longer_than_asked() {
    ffmpeg_next::init().unwrap();
    let addr = http::serve(variants(), None);
    let url = format!("http://{addr}/master.m3u8");
    let fetched = fetch(
        &url,
        ffmpeg_next::Dictionary::new(),
        MASTER.len() - 1,
        || false,
    );
    assert!(fetched.unwrap().is_none());
}

#[test]
fn fetching_fails_saying_why() {
    ffmpeg_next::init().unwrap();
    let addr = http::serve(variants(), None);
    let url = format!("http://{addr}/missing.m3u8");
    let failed = fetch(&url, ffmpeg_next::Dictionary::new(), 1 << 20, || false);
    let err = failed.err().unwrap().to_string();
    assert!(err.contains("404"), "{err}");
}

#[test]
fn fetching_stops_when_told_to() {
    ffmpeg_next::init().unwrap();
    // A server that never answers.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/hang.m3u8", listener.local_addr().unwrap());
    let start = Instant::now();
    let fetched = fetch(&url, ffmpeg_next::Dictionary::new(), 1 << 20, || {
        start.elapsed() > Duration::from_millis(200)
    });
    assert!(fetched.is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    drop(listener);
}
