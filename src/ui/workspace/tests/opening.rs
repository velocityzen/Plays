use std::fs;
use std::path::PathBuf;

use super::*;
use crate::files::test_folder;

fn file(name: &str) -> Source {
    Source::File(PathBuf::from(name))
}

fn one(name: &str) -> Listed {
    Listed::One(file(name))
}

fn list(from: &str, names: &[&str]) -> Listed {
    Listed::List {
        from: Some(PathBuf::from(from)),
        sources: names.iter().map(|name| file(name)).collect(),
    }
}

fn block(first: &str, playlist: &[&str], from: Option<&str>) -> Block {
    Block {
        source: file(first),
        playlist: playlist.iter().map(|name| file(name)).collect(),
        from: from.map(PathBuf::from),
    }
}

#[test]
fn a_playlist_file_opens_in_a_block_of_its_own_beside_the_rest() {
    let listed = vec![
        one("a.mp4"),
        list("l.m3u", &["b.mp3", "c.mp3"]),
        one("d.mp4"),
    ];
    assert_eq!(
        blocks(listed, false),
        [
            block("a.mp4", &[], None),
            block("b.mp3", &["b.mp3", "c.mp3"], Some("l.m3u")),
            block("d.mp4", &[], None),
        ]
    );
}

#[test]
fn in_turn_a_playlist_file_s_files_play_among_the_rest() {
    let listed = vec![one("a.mp4"), list("l.m3u", &["b.mp3", "c.mp3"])];
    assert_eq!(
        blocks(listed, true),
        [block("a.mp4", &["a.mp4", "b.mp3", "c.mp3"], None)]
    );
    // Alone, it's the playlist file that's noted as opened.
    assert_eq!(
        blocks(vec![list("l.m3u", &["b.mp3", "c.mp3"])], true),
        [block("b.mp3", &["b.mp3", "c.mp3"], Some("l.m3u"))]
    );
    assert_eq!(blocks(Vec::new(), true), []);
}

#[test]
fn playlist_files_are_read_folders_listed_and_the_rest_left_alone() {
    let dir = test_folder("opening-playlists");
    let album = dir.join("album");
    fs::create_dir_all(&album).unwrap();
    for name in ["1.mp3", "2.mp4"] {
        fs::write(album.join(name), b"").unwrap();
    }
    let playlist = dir.join("list.pls");
    fs::write(
        &playlist,
        "[playlist]\nFile1=album/2.mp4\nFile2=album/1.mp3\n",
    )
    .unwrap();
    let empty = dir.join("empty.m3u");
    fs::write(&empty, "#EXTM3U\n").unwrap();
    let hls = dir.join("live.m3u8");
    fs::write(&hls, "#EXTM3U\n#EXT-X-TARGETDURATION:6\nsegment.ts\n").unwrap();
    let stream = Source::Url("https://cdn.example/live/index.m3u8".to_owned());

    let given = [&album, &playlist, &empty, &hls].map(|path| Source::File(path.clone()));
    let mut sources = given.to_vec();
    sources.push(stream.clone());
    assert_eq!(
        expand(sources, Some(file_types::Kind::Audio)),
        [
            Listed::One(Source::File(album.join("1.mp3"))),
            Listed::List {
                from: Some(playlist),
                sources: vec![
                    Source::File(album.join("2.mp4")),
                    Source::File(album.join("1.mp3")),
                ],
            },
            Listed::One(Source::File(empty)),
            Listed::One(Source::File(hls)),
            Listed::One(stream),
        ]
    );
}
