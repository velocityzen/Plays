use super::*;

/// A folder of its own holding `files`, empty ones, and `song.m4a`, the
/// file whose cover is looked for.
fn folder(name: &str, files: &[&str]) -> PathBuf {
    let dir = crate::files::test_folder(&format!("cover-files-{name}"));
    fs::create_dir_all(&dir).unwrap();
    for file in files.iter().chain(&["song.m4a"]) {
        fs::write(dir.join(file), b"").unwrap();
    }
    dir.join("song.m4a")
}

/// The name of the cover found beside `files`.
fn found(name: &str, files: &[&str]) -> Option<String> {
    find(&folder(name, files)).map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
}

#[test]
fn each_extension_is_a_cover_named_in_any_case() {
    for (i, cover) in [
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "cover.tif",
        "cover.tiff",
        "Cover.JPG",
        "COVER.Png",
        "cover.TIFF",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            found(&format!("each-{i}"), &[cover]).as_deref(),
            Some(cover)
        );
    }
}

#[test]
fn the_first_extension_of_those_beside_it_is_picked() {
    let files = [
        "cover.tiff",
        "cover.tif",
        "cover.png",
        "cover.jpeg",
        "cover.jpg",
    ];
    for (i, picked) in files.iter().enumerate() {
        assert_eq!(
            found(&format!("first-{i}"), &files[..=i]).as_deref(),
            Some(*picked),
            "of {:?}",
            &files[..=i]
        );
    }
}

#[test]
fn other_pictures_and_names_are_no_cover() {
    let others = [
        "cover.gif",
        "cover.webp",
        "cover",
        "folder.jpg",
        "front.png",
        "covers.jpg",
        "my cover.jpg",
        "cover.jpg.txt",
        "song.jpg",
        ".jpg",
    ];
    assert_eq!(found("others", &others), None);
}

#[test]
fn a_folder_named_like_a_cover_is_none() {
    let song = folder("a-folder", &[]);
    fs::create_dir_all(song.with_file_name("cover.jpg")).unwrap();
    assert_eq!(find(&song), None);
}
