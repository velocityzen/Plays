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
    let dir = std::env::temp_dir()
        .join("plays-tests")
        .join("find-external");
    let _ = fs::remove_dir_all(&dir);
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

#[test]
fn tells_text_encodings() {
    let dir = std::env::temp_dir().join("plays-tests").join("encodings");
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
