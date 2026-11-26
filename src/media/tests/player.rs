use std::fs;
use std::path::Path;
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

#[test]
fn a_live_stream_paused_plays_on_from_where_it_is_now() {
    let made = stream_media().join("hls");
    let folder = test_folder("live-edge");
    fs::create_dir_all(&folder).unwrap();
    for n in 0..4 {
        let segment = format!("segment{n}.ts");
        fs::copy(made.join(&segment), folder.join(&segment)).unwrap();
    }
    live(&folder, 0, 1);
    let addr = http::serve(folder.clone(), None);
    let source = Source::parse_url(&format!("http://{addr}/live.m3u8")).unwrap();
    let mut player = Player::open(source, None, options(true, false));
    let started = wait_until(Duration::from_secs(10), || {
        player.tick();
        player.has_started() && player.frame_pts().is_some()
    });
    assert!(started, "{:?}", player.error());
    assert!(player.is_live());
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
    let back = wait_until(Duration::from_secs(10), || {
        player.tick();
        !player.is_buffering()
    });
    assert!(back, "{:?}", player.error());
    let now = player.frame_pts().unwrap();
    assert!(now > 1.9, "from the third segment, not {now}");
    assert!(player.position() > 1.9, "at {}", player.position());
}
