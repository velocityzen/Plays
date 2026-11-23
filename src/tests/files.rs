use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::*;

#[test]
fn a_tests_folder_goes_as_the_test_ends_whether_it_passed_or_failed() {
    for fails in [false, true] {
        let (sent, made) = mpsc::channel();
        // A test of its own, on a thread of its own as each test is.
        let ended = thread::spawn(move || {
            let folder = Folder::for_test("gone");
            let left = folder.file("left.json").unwrap();
            write_whole(&left, b"{}").unwrap();
            // Named for the test again, it's empty again.
            let folder = Folder::for_test("gone");
            assert!(!left.exists(), "what was left there is gone");
            let path = folder.file("settings.json").unwrap();
            write_whole(&path, b"{}").unwrap();
            sent.send(path).unwrap();
            assert!(!fails, "the test failed");
        })
        .join();
        assert_eq!(ended.is_err(), fails);
        let path = made.recv().unwrap();
        let folder = path.parent().unwrap();
        assert!(folder.starts_with(std::env::temp_dir().join("plays-tests")));
        assert!(!folder.exists(), "{} is left", folder.display());
    }
}

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
    let folder = test_folder("files");
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
}

/// Long enough for a thread that doesn't wait to have done what it does.
const MOMENT: Duration = Duration::from_millis(50);

/// The latest contents waiting are written after a write under way, of
/// older ones, not before it, to be written over.
#[test]
fn a_file_is_written_after_a_write_under_way() {
    let path = test_folder("files-after").join("progress.json");
    let writer = Writer::default();
    let under_way = writer.0.writing.lock();
    writer.0.files.lock().insert(path.clone(), b"new".to_vec());
    let writing = thread::spawn({
        let writer = writer.clone();
        move || writer.write()
    });
    thread::sleep(MOMENT);
    write_whole(&path, b"old").unwrap();
    drop(under_way);
    writing.join().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
}

/// A file removed while it's being written goes once it's written, not
/// before, to be put back.
#[test]
fn a_file_is_removed_after_a_write_under_way() {
    let path = test_folder("files-remove").join("progress.json");
    let writer = Writer::default();
    let under_way = writer.0.writing.lock();
    let removing = thread::spawn({
        let (writer, path) = (writer.clone(), path.clone());
        move || writer.remove(&path)
    });
    thread::sleep(MOMENT);
    write_whole(&path, b"old").unwrap();
    drop(under_way);
    removing.join().unwrap().unwrap();
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn a_private_file_is_its_owners_alone_though_one_left_beside_it_was_not() {
    use std::os::unix::fs::PermissionsExt;

    let folder = test_folder("private");
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
