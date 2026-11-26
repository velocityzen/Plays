use std::fs;
use std::path::{Path, PathBuf};

use url::Url;

use super::*;
use crate::files::test_folder;
use crate::media::tests::http;

fn file(path: &str) -> Source {
    Source::File(PathBuf::from(path))
}

fn stream(url: &str) -> Source {
    Source::Url(url.to_owned())
}

#[test]
fn names_m3u_and_pls_files_in_any_case() {
    for name in ["a.m3u", "a.M3U8", "a.pls", "a.PLS"] {
        assert!(is_playlist_file(Path::new(name)), "{name}");
    }
    for name in ["a.mp3", "m3u", "a.m3u.mp4"] {
        assert!(!is_playlist_file(Path::new(name)), "{name}");
    }
}

#[test]
fn reads_only_m3u_and_pls_urls_over_http() {
    assert!(is_playlist_url("http://radio.example/listen.pls"));
    assert!(is_playlist_url("https://radio.example/a/B.M3U?token=1"));
    assert!(!is_playlist_url("https://cdn.example/live/index.m3u8"));
    assert!(!is_playlist_url("rtmp://radio.example/listen.pls"));
    assert!(!is_playlist_url("http://radio.example/listen.pls.mp3"));
}

#[test]
fn m3u_lists_its_lines_but_comments_and_blanks() {
    let text = "#EXTM3U\r\n\
        #EXTINF:123,Artist - Title\r\n\
        one.mp3\r\n\
        \r\n\
        \t  two folder/two.flac  \r\n\
        # a comment\r\n\
        http://radio.example/stream\r\n";
    assert_eq!(
        entries(text).unwrap(),
        [
            "one.mp3",
            "two folder/two.flac",
            "http://radio.example/stream"
        ]
    );
    assert_eq!(entries("").unwrap(), Vec::<String>::new());
}

#[test]
fn an_hls_playlist_lists_nothing_as_it_s_a_stream() {
    let media = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nsegment0.ts\n";
    assert_eq!(entries(media), None);
    let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nlow/index.m3u8\n";
    assert_eq!(entries(master), None);
}

#[test]
fn pls_lists_its_files_by_number() {
    let text = "\u{feff}[playlist]\n\
        NumberOfEntries=3\n\
        File2=second.mp3\n\
        Title2=Second\n\
        file10 = tenth.mp3\n\
        File1=http://radio.example:8000/\n\
        Length1=-1\n\
        File1=again.mp3\n\
        File=none.mp3\n\
        File3=\n\
        Version=2\n";
    assert_eq!(
        entries(text.trim_start_matches('\u{feff}')).unwrap(),
        ["http://radio.example:8000/", "second.mp3", "tenth.mp3"]
    );
    assert_eq!(
        entries("[Playlist]\nFile1=a.mp3\n").unwrap(),
        ["a.mp3"],
        "in any case"
    );
}

#[test]
fn text_is_read_as_its_mark_says_else_utf8_else_as_it_looks() {
    assert_eq!(text("é.mp3".as_bytes()), "é.mp3");
    assert_eq!(text(b"\xef\xbb\xbf\xc3\xa9.mp3"), "é.mp3");
    let utf16: Vec<u8> = [0xff, 0xfe]
        .into_iter()
        .chain("é.mp3".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    assert_eq!(text(&utf16), "é.mp3");
    // Windows-1252, as older players wrote .m3u files.
    assert_eq!(
        text(b"Caf\xe9 de la Gare - Chanson fran\xe7aise.mp3"),
        "Café de la Gare - Chanson française.mp3"
    );
}

#[test]
fn entries_name_files_from_the_playlist_s_folder() {
    let folder = Base::Folder(Path::new("/music/lists"));
    assert_eq!(resolve("a.mp3", &folder), Some(file("/music/lists/a.mp3")));
    assert_eq!(
        resolve("../albums/b.flac", &folder),
        Some(file("/music/lists/../albums/b.flac"))
    );
    assert_eq!(resolve("/else/c.mp3", &folder), Some(file("/else/c.mp3")));
    assert_eq!(
        resolve("file:///else/d%20e.mp3", &folder),
        Some(file("/else/d e.mp3"))
    );
    assert_eq!(
        resolve("FILE:///else/f.mp3", &folder),
        Some(file("/else/f.mp3"))
    );
    assert_eq!(
        resolve("https://radio.example/live", &folder),
        Some(stream("https://radio.example/live"))
    );
    assert_eq!(resolve("gopher://radio.example/", &folder), None);
}

#[cfg(not(windows))]
#[test]
fn windows_paths_name_files_here_but_for_their_drives() {
    let folder = Base::Folder(Path::new("/music"));
    assert_eq!(
        resolve(r"Artist\Album\a.mp3", &folder),
        Some(file("/music/Artist/Album/a.mp3"))
    );
    assert_eq!(resolve(r"C:\Music\a.mp3", &folder), None);
    assert_eq!(resolve("c:/Music/a.mp3", &folder), None);
    assert_eq!(resolve(r"\\server\share\a.mp3", &folder), None);
}

#[test]
fn entries_of_a_playlist_from_the_network_name_streams_from_where_it_is() {
    let base = Url::parse("http://radio.example/lists/all.m3u").unwrap();
    let base = Base::Url(&base);
    assert_eq!(
        resolve("one.mp3", &base),
        Some(stream("http://radio.example/lists/one.mp3"))
    );
    assert_eq!(
        resolve("/two.mp3", &base),
        Some(stream("http://radio.example/two.mp3"))
    );
    assert_eq!(
        resolve(r"sub\three.mp3", &base),
        Some(stream("http://radio.example/lists/sub/three.mp3"))
    );
    assert_eq!(
        resolve("rtmp://other.example/live", &base),
        Some(stream("rtmp://other.example/live"))
    );
}

#[test]
fn a_playlist_file_lists_its_files_folders_playlists_and_streams() {
    let dir = test_folder("playlist-file");
    let album = dir.join("album");
    fs::create_dir_all(&album).unwrap();
    for name in ["1.mp3", "2.flac", "cover.jpg"] {
        fs::write(album.join(name), b"").unwrap();
    }
    fs::write(dir.join("inner.pls"), "[playlist]\nFile1=album/2.flac\n").unwrap();
    let list = dir.join("list.m3u");
    fs::write(
        &list,
        "#EXTM3U\nalbum/1.mp3\ninner.pls\nalbum\nhttps://radio.example/live\nmissing.mp3\n",
    )
    .unwrap();
    assert_eq!(
        read(&list).unwrap(),
        [
            Source::File(album.join("1.mp3")),
            Source::File(album.join("2.flac")),
            Source::File(album.join("1.mp3")),
            Source::File(album.join("2.flac")),
            stream("https://radio.example/live"),
            Source::File(dir.join("missing.mp3")),
        ]
    );

    let hls = dir.join("live.m3u8");
    fs::write(&hls, "#EXTM3U\n#EXT-X-TARGETDURATION:6\nsegment.ts\n").unwrap();
    assert_eq!(read(&hls), None, "an HLS playlist is a stream");
    assert_eq!(read(&dir.join("gone.m3u")), None);
}

#[test]
fn playlists_listing_themselves_are_followed_only_so_far() {
    let dir = test_folder("playlist-loop");
    fs::create_dir_all(&dir).unwrap();
    let list = dir.join("loop.m3u");
    fs::write(&list, "a.mp3\nloop.m3u\n").unwrap();
    let listed = read(&list).unwrap();
    assert_eq!(listed.len(), MAX_DEPTH + 2);
    assert_eq!(listed.last(), Some(&Source::File(list)));
}

#[test]
fn a_playlist_over_http_lists_its_streams_from_where_it_came_from() {
    ffmpeg_next::init().unwrap();
    let dir = test_folder("playlist-served");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("radio.pls"),
        "[playlist]\nFile1=low.mp3\nFile2=http://other.example/high\n",
    )
    .unwrap();
    let addr = http::serve(dir, None);
    assert_eq!(
        fetch(&format!("http://{addr}/radio.pls")).unwrap(),
        [
            stream(&format!("http://{addr}/low.mp3")),
            stream("http://other.example/high"),
        ]
    );
    assert_eq!(fetch(&format!("http://{addr}/missing.pls")), None);
}
