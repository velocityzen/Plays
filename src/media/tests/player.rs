use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::files::test_folder;
use crate::media::tests::{http, options, stream_media, wait_until};

/// A live HLS playlist of the stream's second-long segments `first` to
/// `last`, as a server has it at some moment.
fn live(folder: &Path, first: usize, last: usize) {
    let mut text = format!(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:{first}\n"
    );
    for n in first..=last {
        text.push_str(&format!("#EXTINF:1.0,\nsegment{n}.ts\n"));
    }
    fs::write(folder.join("live.m3u8"), text).unwrap();
}

/// A folder of the stream's segments, served, its live playlist listing
/// `first` to `last`; and a player playing it, once it has started.
fn playing_live(test: &str, first: usize, last: usize) -> (PathBuf, Player) {
    let made = stream_media().join("hls");
    let folder = test_folder(test);
    fs::create_dir_all(&folder).unwrap();
    for n in 0..4 {
        let segment = format!("segment{n}.ts");
        fs::copy(made.join(&segment), folder.join(&segment)).unwrap();
    }
    live(&folder, first, last);
    let addr = http::serve(folder.clone(), None);
    let source = Source::parse_url(&format!("http://{addr}/live.m3u8")).unwrap();
    let mut player = Player::open(source, None, options(true, false));
    let started = wait_until(Duration::from_secs(10), || {
        player.tick();
        player.has_started() && player.frame_pts().is_some()
    });
    assert!(started, "{:?}", player.error());
    assert!(player.is_live());
    (folder, player)
}

/// Tick `player` until it has what it waits for; where it shows.
#[track_caller]
fn landed(player: &mut Player) -> f64 {
    let landed = wait_until(Duration::from_secs(10), || {
        player.tick();
        !player.is_buffering() && player.frame_pts().is_some()
    });
    assert!(
        landed,
        "{:?}, buffering {}, at {:?}",
        player.error(),
        player.is_buffering(),
        player.frame_pts()
    );
    player.frame_pts().unwrap()
}

#[test]
fn a_live_stream_paused_plays_on_from_where_it_is_now() {
    let (folder, mut player) = playing_live("live-edge", 0, 1);
    player.pause();
    let paused = player.frame_pts().unwrap();
    assert!(paused < 1.0, "paused in the first segment, at {paused}");

    // While it's paused, the stream goes on.
    live(&folder, 2, 3);
    player.play();
    assert!(
        player.is_buffering(),
        "it waits for where the stream is now"
    );
    let now = landed(&mut player);
    assert!(now > 1.9, "from the third segment, not {now}");
    assert!(player.position() > 1.9, "at {}", player.position());
}

#[test]
fn a_live_stream_seeks_back_within_what_its_server_keeps_and_on_to_live() {
    // Opened three segments from the live edge: the first is before it.
    let (_folder, mut player) = playing_live("live-seeks", 0, 3);
    let opened = player.frame_pts().unwrap();
    assert!(
        opened.abs() < 0.1,
        "its time starts where it opened, not {opened}"
    );
    assert!(player.seeks_back());
    assert!(player.behind_live().unwrap() < LIVE_NEAR, "it plays live");

    player.seek(-0.5);
    let back = landed(&mut player);
    assert!(
        (back + 0.5).abs() < 0.1,
        "half a second before it opened, not {back}"
    );
    assert!(player.behind_live().unwrap() > 0.4);

    // No further back than the server keeps: where it lands, it says.
    player.seek(-100.0);
    let first = landed(&mut player);
    assert!(
        (first + 1.0).abs() < 0.1,
        "the first segment's start, not {first}"
    );
    assert!(
        (player.position() - first).abs() < 0.3,
        "at {}",
        player.position()
    );

    // Past the live edge, it's live again.
    player.seek(100.0);
    assert!(player.is_buffering(), "it opens the stream again");
    let live = landed(&mut player);
    assert!((live - opened).abs() < 0.1, "where it opened, not {live}");
    assert!(player.behind_live().unwrap() < LIVE_NEAR);
}

#[test]
fn a_live_stream_paused_behind_live_plays_on_from_where_it_paused() {
    let (_folder, mut player) = playing_live("live-behind", 0, 3);
    let opened = player.frame_pts().unwrap();
    player.seek(-0.5);
    let back = landed(&mut player);
    player.pause();
    player.play();
    assert!(!player.is_buffering(), "it plays on as it was");
    let on = landed(&mut player);
    assert!(on - back < 0.2, "from {back}, not {on}");
    assert!(player.behind_live().unwrap() > 0.4);

    player.go_live();
    assert!(player.is_buffering(), "and goes live when asked");
    let live = landed(&mut player);
    assert!((live - opened).abs() < 0.1, "where it opened, not {live}");
}
