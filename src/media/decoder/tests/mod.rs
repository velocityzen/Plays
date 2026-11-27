use std::fs;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use super::open_input;
use crate::files;
use crate::media::source::Source;

#[test]
fn a_playlist_on_disk_reaches_no_network() {
    ffmpeg_next::init().unwrap();
    // Each connection counted and cut off, so one fails rather than waits.
    let listening = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listening.local_addr().unwrap().port();
    let connected = Arc::new(AtomicUsize::new(0));
    let counting = Arc::clone(&connected);
    thread::spawn(move || {
        for stream in listening.incoming() {
            counting.fetch_add(1, Ordering::SeqCst);
            drop(stream);
        }
    });
    let folder = std::env::temp_dir().join(format!("plays-network-{}", std::process::id()));
    let folder = files::gone_as_test_ends(folder);
    fs::create_dir_all(&folder).unwrap();
    let list = folder.join("list.m3u8");
    let segment = format!("http://127.0.0.1:{port}/segment.ts");
    fs::write(
        &list,
        format!("#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXTINF:10,\n{segment}\n#EXT-X-ENDLIST\n"),
    )
    .unwrap();
    assert!(open_input(&Source::File(list), None, || false).is_err());
    assert_eq!(connected.load(Ordering::SeqCst), 0);
}
