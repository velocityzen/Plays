use super::*;

#[test]
fn parses_stream_urls() {
    assert_eq!(
        Source::parse_url("  HTTPS://example.com/live/index.m3u8 "),
        Ok(Source::Url("https://example.com/live/index.m3u8".into()))
    );
    assert!(Source::parse_url("rtsp://10.0.0.2:554/stream1").is_ok());
    assert!(Source::parse_url("").is_err());
    assert!(Source::parse_url("example.com/video.mp4").is_err());
    assert!(Source::parse_url("file:///etc/passwd").is_err());
    assert!(Source::parse_url("https://").is_err());
    assert!(Source::parse_url("https://example.com/a b").is_err());
}

#[test]
fn treats_arguments_as_files_unless_they_are_urls() {
    assert_eq!(
        Source::from_arg("movie.mkv".into()),
        Source::File("movie.mkv".into())
    );
    assert!(Source::from_arg("rtmp://example.com/app/key".into()).is_network());
}

#[test]
fn opens_file_urls_from_the_system() {
    assert_eq!(
        Source::from_open_url("file:///Users/me/My%20Movies/clip%231.mkv"),
        Some(Source::File("/Users/me/My Movies/clip#1.mkv".into()))
    );
    assert_eq!(
        Source::from_open_url("file://localhost/tmp/song.mp3"),
        Some(Source::File("/tmp/song.mp3".into()))
    );
    assert!(Source::from_open_url("https://example.com/live.m3u8").is_some_and(|s| s.is_network()));
    assert_eq!(Source::from_open_url("mailto:me@example.com"), None);
}

#[test]
fn finds_the_media_files_before_and_after_a_file_and_all_of_them() {
    let folder = std::env::temp_dir().join(format!("plays-neighbors-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    for name in ["a.mkv", "b.txt", "c.mp3", ".d.mp4", "e.MP4"] {
        std::fs::write(folder.join(name), b"").unwrap();
    }
    let neighbors = |name: &str| Source::File(folder.join(name)).neighbors();
    assert_eq!(
        neighbors("c.mp3"),
        Neighbors {
            previous: Some(folder.join("a.mkv")),
            next: Some(folder.join("e.MP4")),
            all: ["a.mkv", "c.mp3", "e.MP4"]
                .map(|name| folder.join(name))
                .into(),
        }
    );
    assert_eq!(neighbors("a.mkv").previous, None);
    assert_eq!(neighbors("e.MP4").next, None);
    // A file that isn't media still has the ones around it by name.
    assert_eq!(neighbors("b.txt").next, Some(folder.join("c.mp3")));
    assert_eq!(
        Source::Url("https://example.com/live.m3u8".into()).neighbors(),
        Neighbors::default()
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn lists_media_files_and_links_to_them_but_not_folders() {
    let folder = std::env::temp_dir().join(format!("plays-media-in-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(folder.join("season.mkv")).unwrap();
    for name in ["b.mp4", "notes.txt"] {
        std::fs::write(folder.join(name), b"").unwrap();
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(folder.join("b.mp4"), folder.join("a.mkv")).unwrap();
        std::os::unix::fs::symlink(folder.join("season.mkv"), folder.join("c.mkv")).unwrap();
        std::os::unix::fs::symlink(folder.join("gone.mp4"), folder.join("d.mp4")).unwrap();
    }
    let names: Vec<_> = media_in(&folder)
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let expected: &[&str] = if cfg!(unix) {
        &["a.mkv", "b.mp4"]
    } else {
        &["b.mp4"]
    };
    assert_eq!(names, expected);
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn names_and_displays_urls_without_secrets() {
    let source =
        Source::Url("https://user:secret@cdn.example.com:8443/live/cam1.m3u8?token=abc".into());
    assert_eq!(source.name(), "cdn.example.com:8443/cam1.m3u8");
    assert_eq!(
        source.to_string(),
        "https://cdn.example.com:8443/live/cam1.m3u8"
    );
    assert_eq!(
        Source::Url("rtmp://example.com".into()).name(),
        "example.com"
    );
    assert_eq!(Source::File("/videos/clip.mp4".into()).name(), "clip.mp4");
}
