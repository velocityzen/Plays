use std::ffi::CStr;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use ffmpeg_next::{Dictionary, ffi, format};

use super::{input_file, input_file_with, input_stream, network_options};
use crate::tag;

/// A fixture of the workspace's.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// A folder of the test's own, removed as it ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let folder =
            std::env::temp_dir().join(format!("ffmpeg-safe-{name}-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        Self(folder)
    }

    /// An HLS playlist in it, of the one segment at `segment`.
    fn playlist(&self, segment: &str) -> PathBuf {
        let path = self.0.join("list.m3u8");
        let list = format!(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n\
             #EXTINF:10,\n{segment}\n#EXT-X-ENDLIST\n"
        );
        fs::write(&path, list).unwrap();
        path
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn opens_a_file() {
    let input = input_file(&fixture("clip.mp4")).expect("the fixture");
    assert!(input.streams().count() > 0);
}

/// A port on this Mac that counts who connects, each cut off at once, so
/// what connects fails rather than waits.
struct Watched {
    port: u16,
    connected: Arc<AtomicUsize>,
}

impl Watched {
    fn new() -> Self {
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
        Self { port, connected }
    }

    fn connected(&self) -> usize {
        self.connected.load(Ordering::SeqCst)
    }
}

/// Options letting what's opened reach the network.
fn to_the_network() -> Dictionary<'static> {
    let mut options = Dictionary::new();
    options.set("protocol_whitelist", "file,http,tcp");
    options
}

#[test]
fn a_playlist_on_disk_reaches_no_network() {
    let watched = Watched::new();
    let folder = Folder::new("network");
    let list = folder.playlist(&format!("http://127.0.0.1:{}/segment.ts", watched.port));
    assert!(input_file(&list).is_err(), "nothing to play");
    let opened = input_file_with(&list, || false, to_the_network());
    assert!(opened.is_err(), "not widened by what's asked");
    assert_eq!(watched.connected(), 0);

    // As FFmpeg opens it, asked to.
    assert!(format::input_with_dictionary(&list, to_the_network()).is_err());
    assert_eq!(watched.connected(), 1, "it would have");
}

#[test]
fn a_playlist_on_disk_plays_files_on_disk() {
    let folder = Folder::new("files");
    let segment = fixture("tune.flac");
    let list = folder.playlist(&segment.to_string_lossy());
    let input = input_file(&list).expect("the playlist");
    assert!(input.streams().count() > 0);
}

#[test]
fn a_stream_is_opened_waiting_15_seconds_and_connecting_again() {
    let options = network_options();
    let set = |key: &CStr| tag(&options, key);
    assert_eq!(set(c"rw_timeout").as_deref(), Some("15000000"));
    assert_eq!(set(c"timeout").as_deref(), Some("15000000"));
    assert_eq!(set(c"reconnect").as_deref(), Some("1"));
    assert_eq!(set(c"reconnect_streamed").as_deref(), Some("1"));
    assert_eq!(set(c"reconnect_delay_max").as_deref(), Some("5"));
    assert_eq!(set(c"rtsp_transport").as_deref(), Some("tcp"));
}

#[test]
fn a_stream_reaches_the_network_until_told_to_stop() {
    let watched = Watched::new();
    let url = format!("http://127.0.0.1:{}/stream.ts", watched.port);
    assert!(input_stream(&url, || true).is_err(), "told to stop");
    assert_eq!(watched.connected(), 0, "before connecting");
    assert!(input_stream(&url, || false).is_err(), "cut off");
    assert!(watched.connected() >= 1);
}

#[test]
fn ffmpeg_has_only_the_protocols_plays_opens() {
    let mut found = Vec::new();
    let mut place = ptr::null_mut();
    loop {
        // SAFETY: FFmpeg keeps its place in `place`, and names each of
        // its protocols with a string it keeps.
        let name = unsafe { ffi::avio_enum_protocols(&mut place, 0) };
        if name.is_null() {
            break;
        }
        // SAFETY: as above.
        found.push(unsafe { CStr::from_ptr(name) }.to_str().unwrap());
    }
    // RTMPE's encryption needs OpenSSL, so isn't on macOS.
    let opened = [
        "file",
        "crypto",
        "data",
        "http",
        "https",
        "httpproxy",
        "tcp",
        "tls",
        "udp",
        "rtp",
        "srtp",
        "rtmp",
        "rtmps",
        "rtmpe",
        "rtmpt",
        "rtmpte",
        "rtmpts",
        "ffrtmpcrypt",
        "ffrtmphttp",
        "mmsh",
        "mmst",
        "ftp",
    ];
    let more: Vec<_> = found.iter().filter(|name| !opened.contains(name)).collect();
    assert!(more.is_empty(), "built with {more:?} too");
    for needed in [
        "file", "crypto", "data", "https", "rtmp", "rtmpt", "rtp", "udp",
    ] {
        assert!(found.contains(&needed), "built without {needed}");
    }
}
