use gpui::TestAppContext;

use super::*;
use crate::files;
use crate::media::picking::Subtitles;

const DAY: u64 = 24 * 60 * 60;

/// `file` at `position` of `duration`, with no tracks picked.
fn at(file: &Path, position: f64, duration: f64) -> Note {
    Note {
        file: file.to_owned(),
        position,
        duration,
        picked: Picked::default(),
    }
}

/// `file` at `position` of `duration`, with `picked`.
fn with(file: &Path, position: f64, duration: f64, picked: Picked) -> Note {
    Note {
        picked,
        ..at(file, position, duration)
    }
}

/// The app, as far as places go: Settings as they start out, which
/// resume files, and the places loaded from `folder`, kept there.
fn start(cx: &mut TestAppContext, folder: &Folder) {
    cx.update(|cx| {
        cx.set_global(folder.clone());
        cx.set_global(Settings::default());
        cx.set_global(Progress::load(folder));
    });
}

/// Save the places, noting none, and let them be written.
fn save(cx: &mut TestAppContext) {
    cx.update(|cx| note(std::iter::empty(), true, cx));
    cx.run_until_parked();
}

#[test]
fn a_file_barely_begun_or_nearly_over_starts_over() {
    let movie = 2.0 * 60.0 * 60.0;
    assert!(!worth_resuming(5.0, movie), "barely begun");
    assert!(worth_resuming(10.0, movie));
    assert!(worth_resuming(movie * 0.95, movie));
    assert!(!worth_resuming(movie * 0.96, movie), "the credits");
    // A short one: a twentieth of it is begun.
    assert!(worth_resuming(1.0, 2.0));
    assert!(!worth_resuming(0.05, 2.0));
    assert!(!worth_resuming(1.95, 2.0));
    assert!(!worth_resuming(0.0, 0.0), "no length");
}

#[test]
fn a_place_is_kept_for_thirty_days_since_it_last_played() {
    let mut progress = Progress::default();
    let file = Path::new("/videos/movie.mkv");
    progress.note(&at(file, 600.0, 7200.0), 0);
    assert_eq!(progress.recall(file, 0), Some(600.0));
    assert!(progress.changed);

    progress.changed = false;
    progress.note(&at(file, 600.0, 7200.0), 10 * DAY);
    assert!(!progress.changed, "not played since: still from then");
    assert_eq!(progress.recall(file, 30 * DAY), Some(600.0));
    assert_eq!(progress.recall(file, 31 * DAY), None, "gone");

    progress.note(&at(file, 900.0, 7200.0), 20 * DAY);
    assert_eq!(progress.recall(file, 31 * DAY), Some(900.0), "played since");
    progress.forget_old(51 * DAY);
    assert!(progress.files.is_empty());

    progress.note(&at(file, 600.0, 7200.0), 0);
    progress.note(&at(file, 7190.0, 7200.0), 0);
    assert_eq!(progress.recall(file, 0), None, "played to its end");
}

#[test]
fn the_tracks_picked_are_kept_with_the_place_or_without_one() {
    let mut progress = Progress::default();
    let file = Path::new("/videos/movie.mkv");
    let picked = Picked {
        audio: Some(2),
        subtitles: Some(Subtitles::Track(4)),
    };
    progress.note(&with(file, 600.0, 7200.0, picked), 0);
    assert_eq!(progress.recall(file, 0), Some(600.0));
    assert_eq!(progress.picked(file, 0), picked);

    progress.changed = false;
    progress.note(&with(file, 600.0, 7200.0, picked), DAY);
    assert!(!progress.changed, "as it was");
    let hidden = Picked {
        subtitles: Some(Subtitles::Off),
        ..picked
    };
    progress.note(&with(file, 600.0, 7200.0, hidden), DAY);
    assert!(progress.changed, "subtitles hidden since");
    assert_eq!(progress.picked(file, 31 * DAY), hidden, "and played then");

    for (position, why) in [(3.0, "barely begun"), (7190.0, "played to its end")] {
        progress.note(&with(file, position, 7200.0, picked), 0);
        assert_eq!(progress.recall(file, 0), None, "{why}: starts over");
        assert_eq!(progress.picked(file, 0), picked, "{why}: with them");
    }
    assert_eq!(progress.picked(file, 31 * DAY), Picked::default(), "gone");
    progress.note(&at(file, 3.0, 7200.0), 0);
    assert!(progress.files.is_empty(), "nothing to keep");
    assert_eq!(progress.picked(file, 0), Picked::default());
}

#[test]
fn places_are_saved_by_whole_path() {
    let mut progress = Progress::default();
    progress.note(&at(Path::new("/videos/movie.mkv"), 600.0, 7200.0), 7);
    let json = serde_json::to_string(&progress.files).unwrap();
    assert_eq!(
        json,
        r#"{"/videos/movie.mkv":{"position":600.0,"played":7}}"#
    );
    let picked = Picked {
        audio: None,
        subtitles: Some(Subtitles::Off),
    };
    progress.note(
        &with(Path::new("/videos/movie.mkv"), 1.0, 7200.0, picked),
        7,
    );
    let json = serde_json::to_string(&progress.files).unwrap();
    assert_eq!(
        json,
        r#"{"/videos/movie.mkv":{"played":7,"picked":{"subtitles":"off"}}}"#
    );
    let read: HashMap<String, Spot> = serde_json::from_str(&json).unwrap();
    assert_eq!(read, progress.files);
    let relative = Path::new("movie.mkv");
    progress.note(&at(relative, 60.0, 7200.0), 7);
    assert!(progress.files.contains_key(&key(relative)));
    assert!(Path::new(&key(relative)).is_absolute());
}

#[gpui::test]
fn places_are_saved_in_the_apps_folder_and_read_back(cx: &mut TestAppContext) {
    let folder = Folder::for_test("progress-saved");
    let path = folder.file(FILE).unwrap();
    start(cx, &folder);
    let movie = Path::new("/videos/movie.mkv");
    cx.update(|cx| note([at(movie, 600.0, 7200.0)], false, cx));
    cx.run_until_parked();
    assert!(!path.exists(), "not saved unless asked");
    save(cx);
    let loaded = Progress::load(&folder);
    assert_eq!(loaded.recall(movie, now()), Some(600.0));
    assert!(!loaded.changed);
}

/// A save is written off the UI thread, as a full flush takes a while:
/// the latest places, those noted before them never written after them.
#[gpui::test]
fn places_are_written_off_the_ui_thread_the_latest_last(cx: &mut TestAppContext) {
    let folder = Folder::for_test("progress-soon");
    let path = folder.file(FILE).unwrap();
    start(cx, &folder);
    let movie = Path::new("/videos/movie.mkv");
    cx.update(|cx| {
        note([at(movie, 600.0, 7200.0)], true, cx);
        note([at(movie, 900.0, 7200.0)], true, cx);
        assert!(!path.exists(), "not on this thread");
    });
    cx.run_until_parked();
    assert_eq!(Progress::load(&folder).recall(movie, now()), Some(900.0));
}

#[gpui::test]
fn places_too_old_are_left_out_of_the_file(cx: &mut TestAppContext) {
    let folder = Folder::for_test("progress-old");
    let path = folder.file(FILE).unwrap();
    let now = now();
    let json = format!(
        r#"{{"/videos/old.mkv":{{"position":60.0,"played":{}}},"/videos/new.mkv":{{"position":90.0,"played":{}}}}}"#,
        now - 31 * DAY,
        now - DAY
    );
    files::write_whole(&path, json.as_bytes()).unwrap();
    start(cx, &folder);
    cx.update(|cx| {
        assert_eq!(recall(Path::new("/videos/old.mkv"), cx), None);
        assert_eq!(recall(Path::new("/videos/new.mkv"), cx), Some(90.0));
    });
    save(cx);
    let saved = fs::read_to_string(&path).unwrap();
    assert!(
        !saved.contains("old.mkv") && saved.contains("new.mkv"),
        "{saved}"
    );
}

#[gpui::test]
fn a_broken_file_is_no_places_and_is_saved_over(cx: &mut TestAppContext) {
    for (name, text) in [
        ("half", r#"{"/videos/movie.mkv":{"position":6"#),
        ("other", "[1, 2]"),
        ("empty", ""),
    ] {
        let folder = Folder::for_test(&format!("progress-{name}"));
        files::write_whole(&folder.file(FILE).unwrap(), text.as_bytes()).unwrap();
        assert!(Progress::load(&folder).files.is_empty(), "{name}");
    }
    let folder = Folder::for_test("progress-half");
    start(cx, &folder);
    let movie = Path::new("/videos/movie.mkv");
    cx.update(|cx| note([at(movie, 600.0, 7200.0)], true, cx));
    cx.run_until_parked();
    assert_eq!(Progress::load(&folder).recall(movie, now()), Some(600.0));
}

#[gpui::test]
fn forgetting_every_place_removes_the_file(cx: &mut TestAppContext) {
    let folder = Folder::for_test("progress-forget");
    let path = folder.file(FILE).unwrap();
    start(cx, &folder);
    let movie = Path::new("/videos/movie.mkv");
    cx.update(|cx| note([at(movie, 600.0, 7200.0)], true, cx));
    cx.run_until_parked();
    assert!(path.exists());
    cx.update(|cx| {
        // One waiting to be written, too.
        note([at(movie, 900.0, 7200.0)], true, cx);
        forget_all(cx);
        assert_eq!(recall(movie, cx), None);
    });
    cx.run_until_parked();
    assert!(!path.exists());
    assert!(Progress::load(&folder).files.is_empty());
}

#[gpui::test]
fn the_tracks_picked_are_saved_and_read_back(cx: &mut TestAppContext) {
    let folder = Folder::for_test("progress-picked");
    start(cx, &folder);
    let movie = Path::new("/videos/movie.mkv");
    let picked = Picked {
        audio: Some(1),
        subtitles: Some(Subtitles::Track(3)),
    };
    cx.update(|cx| {
        note([with(movie, 1.0, 7200.0, picked)], true, cx);
        assert_eq!(super::picked(movie, cx), picked);
        assert_eq!(recall(movie, cx), None);
    });
    cx.run_until_parked();
    let loaded = Progress::load(&folder);
    assert_eq!(loaded.picked(movie, now()), picked);
}

#[gpui::test]
fn no_places_are_kept_when_settings_say_not_to(cx: &mut TestAppContext) {
    let folder = Folder::for_test("progress-off");
    start(cx, &folder);
    let movie = Path::new("/videos/movie.mkv");
    let picked = Picked {
        audio: Some(1),
        subtitles: None,
    };
    cx.update(|cx| {
        note([with(movie, 600.0, 7200.0, picked)], false, cx);
        assert_eq!(super::picked(movie, cx), picked);
        Settings::update(cx, false, |settings| settings.resume_files = false);
        assert_eq!(super::picked(movie, cx), Picked::default(), "nor picks");
        note([at(movie, 600.0, 7200.0)], true, cx);
        assert_eq!(recall(movie, cx), None);
    });
    cx.run_until_parked();
    assert!(!folder.file(FILE).unwrap().exists());
}
