//! Settings → Playback → Resume files where they were left: where each
//! file was when it last played, kept for `KEEP_DAYS` days, so it starts
//! there when opened again. A file barely begun, or played nearly to its
//! end, isn't kept: it starts over. Saved to `progress.json` beside the
//! settings as tiles close or play another file, every `SAVE_EVERY` while
//! files are open, and as the app quits.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{App, Global};
use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// How long a file's place is kept since it last played.
pub const KEEP_DAYS: u64 = 30;
/// How often the places of the files open are noted and saved.
pub const SAVE_EVERY: Duration = Duration::from_secs(10);
/// Before this far in a file is barely begun, or before `MARGIN` of it,
/// whichever comes first.
const BEGUN: f64 = 10.0;
/// The share of a file at either end where it is barely begun, or nearly
/// over.
const MARGIN: f64 = 0.05;

/// Where a file was.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
struct Spot {
    /// Seconds into it.
    position: f64,
    /// When it last played there, in seconds since 1970.
    played: u64,
}

/// Where each file was, by its path.
#[derive(Default)]
pub struct Progress {
    files: HashMap<String, Spot>,
    /// It changed since it was saved.
    changed: bool,
}

impl Global for Progress {}

/// A file's place, as a tile has it: the file, where it is, and how long it
/// is.
pub type Note = (PathBuf, f64, f64);

/// Where the places are saved; nowhere in tests and smoke runs, as with the
/// settings.
fn path() -> Option<PathBuf> {
    if cfg!(any(test, feature = "smoke")) {
        return None;
    }
    dirs::config_dir().map(|dir| dir.join("plays").join("progress.json"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// A file's key: its whole path, however it was opened.
fn key(file: &Path) -> String {
    std::path::absolute(file)
        .unwrap_or_else(|_| file.to_owned())
        .to_string_lossy()
        .into_owned()
}

/// Whether `spot` played within `KEEP_DAYS` of `now`.
fn fresh(spot: &Spot, now: u64) -> bool {
    now.saturating_sub(spot.played) <= KEEP_DAYS * 24 * 60 * 60
}

/// Whether a file at `position` of `duration` starts there again: it isn't
/// barely begun, nor nearly over.
fn worth_resuming(position: f64, duration: f64) -> bool {
    duration > 0.0
        && position >= BEGUN.min(duration * MARGIN)
        && position <= duration * (1.0 - MARGIN)
}

impl Progress {
    /// The places saved, but those too old.
    pub fn load() -> Self {
        let files = path()
            .and_then(|path| {
                let text = fs::read_to_string(&path).ok()?;
                serde_json::from_str(&text)
                    .inspect_err(|err| log::warn!("ignoring {}: {err}", path.display()))
                    .ok()
            })
            .unwrap_or_default();
        let mut progress = Self {
            files,
            changed: false,
        };
        progress.forget_old(now());
        progress
    }

    fn note(&mut self, file: &Path, position: f64, duration: f64, now: u64) {
        let key = key(file);
        if !worth_resuming(position, duration) {
            self.changed |= self.files.remove(&key).is_some();
            return;
        }
        // Left where it was: it didn't play since.
        if self
            .files
            .get(&key)
            .is_some_and(|spot| spot.position == position)
        {
            return;
        }
        self.files.insert(
            key,
            Spot {
                position,
                played: now,
            },
        );
        self.changed = true;
    }

    fn recall(&self, file: &Path, now: u64) -> Option<f64> {
        self.files
            .get(&key(file))
            .filter(|spot| fresh(spot, now))
            .map(|spot| spot.position)
    }

    fn forget_old(&mut self, now: u64) {
        let before = self.files.len();
        self.files.retain(|_, spot| fresh(spot, now));
        self.changed |= self.files.len() != before;
    }

    fn save(&mut self) {
        if !self.changed {
            return;
        }
        self.forget_old(now());
        self.changed = false;
        let Some(path) = path() else {
            return;
        };
        let result = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&path, serde_json::to_string(&self.files)?));
        if let Err(err) = result {
            log::warn!("could not save {}: {err}", path.display());
        }
    }
}

/// Note where files are, as Settings say to, and save the places if
/// `save`.
pub fn note(notes: impl IntoIterator<Item = Note>, save: bool, cx: &mut App) {
    if !cx.global::<Settings>().resume_files {
        return;
    }
    let progress = cx.default_global::<Progress>();
    let now = now();
    for (file, position, duration) in notes {
        progress.note(&file, position, duration, now);
    }
    if save {
        progress.save();
    }
}

/// Where `file` was, if Settings say to resume files and it played within
/// `KEEP_DAYS`.
pub fn recall(file: &Path, cx: &App) -> Option<f64> {
    if !cx.global::<Settings>().resume_files {
        return None;
    }
    cx.try_global::<Progress>()?.recall(file, now())
}

/// Forget every file's place, the saved ones too, as Settings stop keeping
/// them.
pub fn forget_all(cx: &mut App) {
    cx.set_global(Progress::default());
    if let Some(path) = path() {
        fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60;

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
        progress.note(file, 600.0, 7200.0, 0);
        assert_eq!(progress.recall(file, 0), Some(600.0));
        assert!(progress.changed);

        progress.changed = false;
        progress.note(file, 600.0, 7200.0, 10 * DAY);
        assert!(!progress.changed, "not played since: still from then");
        assert_eq!(progress.recall(file, 30 * DAY), Some(600.0));
        assert_eq!(progress.recall(file, 31 * DAY), None, "gone");

        progress.note(file, 900.0, 7200.0, 20 * DAY);
        assert_eq!(progress.recall(file, 31 * DAY), Some(900.0), "played since");
        progress.forget_old(51 * DAY);
        assert!(progress.files.is_empty());

        progress.note(file, 600.0, 7200.0, 0);
        progress.note(file, 7190.0, 7200.0, 0);
        assert_eq!(progress.recall(file, 0), None, "played to its end");
    }

    #[test]
    fn places_are_saved_by_whole_path() {
        let mut progress = Progress::default();
        progress.note(Path::new("/videos/movie.mkv"), 600.0, 7200.0, 7);
        let json = serde_json::to_string(&progress.files).unwrap();
        assert_eq!(
            json,
            r#"{"/videos/movie.mkv":{"position":600.0,"played":7}}"#
        );
        let relative = Path::new("movie.mkv");
        progress.note(relative, 60.0, 7200.0, 7);
        assert!(progress.files.contains_key(&key(relative)));
        assert!(Path::new(&key(relative)).is_absolute());
    }
}
