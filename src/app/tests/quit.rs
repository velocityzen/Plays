//! What the app does as it quits.

use std::fs;
use std::path::Path;

use gpui::TestAppContext;

use super::keep_progress;
use crate::files::Folder;
use crate::progress::{self, Note, Progress};
use crate::settings::Settings;

/// Places waiting to be written off the UI thread are written as the app
/// quits, which won't wait for a thread.
#[gpui::test]
fn places_waiting_are_written_as_the_app_quits(cx: &mut TestAppContext) {
    let folder = Folder::for_test("app-quit-places");
    cx.update(|cx| {
        cx.set_global(folder.clone());
        cx.set_global(Settings::default());
        cx.set_global(Progress::load(&folder));
        keep_progress(cx);
        let note = Note {
            file: Path::new("/videos/movie.mkv").to_owned(),
            position: 600.0,
            duration: 7200.0,
            picked: Default::default(),
        };
        progress::note([note], true, cx);
    });
    cx.quit();
    let saved = fs::read_to_string(folder.file("progress.json").unwrap()).unwrap();
    assert!(saved.contains("movie.mkv"), "{saved}");
}
