use super::*;

fn naming(middle: &str) -> Naming {
    Naming::parse(middle)
}

#[test]
fn reads_names() {
    assert_eq!(naming(""), Naming::default());
    let english = Naming {
        language: Some("en".into()),
        ..Naming::default()
    };
    assert_eq!(naming("en"), english);
    assert_eq!(naming("EN"), english);
    assert_eq!(
        naming("eng.forced"),
        Naming {
            language: Some("eng".into()),
            forced: true,
            ..Naming::default()
        }
    );
    assert_eq!(
        naming("English.SDH"),
        Naming {
            language: Some("eng".into()),
            hearing_impaired: true,
            ..Naming::default()
        }
    );
    assert!(naming("en.hi").hearing_impaired);
    assert_eq!(naming("hi").language.as_deref(), Some("hi"));
    assert_eq!(naming("pt-BR").language.as_deref(), Some("pt-BR"));
    assert_eq!(
        naming("2_English"),
        Naming {
            language: Some("eng".into()),
            ..Naming::default()
        }
    );
    assert_eq!(
        naming("en.Director's_Cut"),
        Naming {
            language: Some("en".into()),
            title: Some("Director's Cut".into()),
            ..Naming::default()
        }
    );
    assert_eq!(naming("Commentary").title.as_deref(), Some("Commentary"));
}

#[test]
fn finds_files_for_a_video() {
    let dir = crate::files::test_folder("find-external");
    fs::create_dir_all(dir.join("Subs").join("movie")).unwrap();
    for name in [
        "Movie.mkv",
        "Movie.srt",
        "movie.en.forced.ASS",
        "Movie 2.srt",
        "Movie.nfo",
        "Other.srt",
        "Movie.de.idx",
        "Movie.de.sub",
        "Movie.fr.sub",
        "Subs/Movie.es.vtt",
        "Subs/movie/2_English.srt",
    ] {
        fs::write(dir.join(name), "").unwrap();
    }
    let found: Vec<(String, Naming)> = find(&dir.join("Movie.mkv"))
        .into_iter()
        .map(|found| {
            let path = found.path.strip_prefix(&dir).unwrap();
            (path.to_string_lossy().into_owned(), found.naming)
        })
        .collect();
    let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Movie.de.idx",
            "movie.en.forced.ASS",
            "Movie.fr.sub",
            "Movie.srt",
            "Subs/Movie.es.vtt",
            "Subs/movie/2_English.srt",
        ]
    );
    assert!(found[1].1.forced);
    assert_eq!(found[3].1, Naming::default());
    assert_eq!(found[5].1.language.as_deref(), Some("eng"));
}

/// On a share far away each look at a file is a trip: only those named
/// for the video, or its folders, are looked at, and the folder's listing
/// says what each is. Only a link is looked at again, to follow it.
#[cfg(unix)]
#[test]
fn only_entries_named_for_it_are_looked_at_and_only_links_again() {
    use std::os::unix::fs::symlink;
    let dir = crate::files::test_folder("find-looked-at");
    let elsewhere = dir.join("elsewhere");
    fs::create_dir_all(elsewhere.join("Subtitles")).unwrap();
    // A folder named as a file is, not one.
    fs::create_dir_all(dir.join("Movie.it.srt")).unwrap();
    for name in [
        "Movie.mkv",
        "Movie.en.srt",
        "elsewhere/French.srt",
        "elsewhere/Notes.txt",
        "elsewhere/Subtitles/Movie.de.srt",
    ] {
        fs::write(dir.join(name), "").unwrap();
    }
    for (to, name) in [
        ("French.srt", "Movie.fr.srt"),
        ("Subtitles", "Subs"),
        // Not a subtitle file's name, another video's, and another folder.
        ("Notes.txt", "Notes.txt"),
        ("French.srt", "Movie 2.srt"),
        ("Subtitles", "Extras"),
    ] {
        symlink(elsewhere.join(to), dir.join(name)).unwrap();
    }
    let found: Vec<PathBuf> = find(&dir.join("Movie.mkv"))
        .into_iter()
        .map(|found| found.path.strip_prefix(&dir).unwrap().to_owned())
        .collect();
    assert_eq!(
        found,
        [
            Path::new("Movie.en.srt"),
            Path::new("Movie.fr.srt"),
            Path::new("Subs/Movie.de.srt"),
        ]
    );
    let mut followed: Vec<PathBuf> = crate::media::source::FOLLOWED
        .lock()
        .unwrap()
        .iter()
        .filter_map(|path| path.strip_prefix(&dir).ok().map(Path::to_owned))
        .collect();
    followed.sort();
    assert_eq!(followed, [Path::new("Movie.fr.srt"), Path::new("Subs")]);
}

#[test]
fn tells_text_encodings() {
    let dir = crate::files::test_folder("encodings");
    fs::create_dir_all(&dir).unwrap();
    let utf8 = dir.join("utf8.srt");
    fs::write(&utf8, "1\n00:00:01,000 --> 00:00:02,000\nПривет, мир\n").unwrap();
    assert_eq!(text_encoding(&utf8), None);
    let (cp1251, _, _) = encoding_rs::WINDOWS_1251
        .encode("1\n00:00:01,000 --> 00:00:02,000\nПривет, как дела? Это субтитры.\n");
    let path = dir.join("cp1251.srt");
    fs::write(&path, &cp1251).unwrap();
    assert_eq!(text_encoding(&path), Some("windows-1251"));
    let bitmap = dir.join("disc.sup");
    fs::write(&bitmap, [0xff, 0xfe, 0x80]).unwrap();
    assert_eq!(text_encoding(&bitmap), None);
}
