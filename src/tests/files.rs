use std::io::Read;

use super::*;

#[test]
fn a_path_shows_the_home_folder_as_a_tilde() {
    let home = Some(Path::new("/home/me"));
    assert_eq!(
        shown_from(Path::new("/home/me/Movies/a.mkv"), home),
        "~/Movies/a.mkv"
    );
    assert_eq!(shown_from(Path::new("/home/me"), home), "~");
    assert_eq!(
        shown_from(Path::new("/home/meg/a.mkv"), home),
        "/home/meg/a.mkv"
    );
    assert_eq!(shown_from(Path::new("/media/a.mkv"), None), "/media/a.mkv");
}

#[test]
fn a_file_is_replaced_whole_and_never_written_over() {
    let folder = std::env::temp_dir()
        .join("plays-tests")
        .join(format!("files-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    let path = folder.join("settings.json");
    write_whole(&path, b"{ \"old\": true }").unwrap();
    // What was there, as a reader that opened it before sees it.
    let mut before = fs::File::open(&path).unwrap();
    write_whole(&path, b"{ \"new\": true }").unwrap();
    let mut seen = String::new();
    before.read_to_string(&mut seen).unwrap();
    assert_eq!(seen, "{ \"old\": true }", "written over where it was");
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ \"new\": true }");
    let left: Vec<_> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["settings.json"], "nothing else left beside it");
    let _ = fs::remove_dir_all(&folder);
}

#[cfg(unix)]
#[test]
fn a_private_file_is_its_owners_alone_though_one_left_beside_it_was_not() {
    use std::os::unix::fs::PermissionsExt;

    let folder = std::env::temp_dir()
        .join("plays-tests")
        .join(format!("private-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    let mode = |name: &str| {
        fs::metadata(folder.join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    // Left by a crash, readable by all.
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("airplay.json.tmp"), b"").unwrap();
    fs::set_permissions(
        folder.join("airplay.json.tmp"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    write_private(&folder.join("airplay.json"), b"{}").unwrap();
    assert_eq!(mode("airplay.json"), 0o600);
    assert_eq!(fs::read(folder.join("airplay.json")).unwrap(), b"{}");
    let _ = fs::remove_dir_all(&folder);
}
