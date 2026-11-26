//! Settings → Playback → Resume files where they were left: where each
//! file was when it last played, and how it was set to play (`Kept`), kept
//! for `KEEP_DAYS` days, so it starts there, as it was, when opened again.
//! A file barely begun, or played nearly to its end, starts over, as it
//! was set to play. Saved to `progress.json` beside the
//! settings as blocks close or play another file, every `SAVE_EVERY` while
//! files are open, and as the app quits: off the UI thread, but as it
//! quits.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{App, Global};
use serde::{Deserialize, Serialize};

use crate::files::Folder;
use crate::media::picking::Picked;
use crate::settings::Settings;
use crate::ui::framing::Adjustments;

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

/// How a file was set to play: the tracks picked in it, how its picture
/// was adjusted, and its delays.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Kept {
    #[serde(skip_serializing_if = "Picked::is_empty")]
    pub picked: Picked,
    #[serde(with = "picture", skip_serializing_if = "picture::is_plain")]
    pub picture: Adjustments,
    /// Seconds the sound is heard after the picture, and subtitles show
    /// after their times (negative: before).
    #[serde(skip_serializing_if = "is_zero")]
    pub audio_delay: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub subtitle_delay: f64,
}

impl Kept {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

/// A picture's adjustments as they're saved: turned and mirrored, and the
/// aspect ratio and crop by the names the menus give them, left out once
/// the menus give none.
mod picture {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::media::Orientation;
    use crate::ui::framing::{ASPECT_RATIOS, Adjustments, CROPS, Ratio};

    #[derive(Default, Serialize, Deserialize)]
    #[serde(default)]
    struct Saved {
        #[serde(skip_serializing_if = "is_upright")]
        turns: u8,
        #[serde(skip_serializing_if = "is_upright")]
        mirrored: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        aspect_ratio: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        crop: Option<String>,
    }

    fn is_upright<T: Default + PartialEq>(value: &T) -> bool {
        *value == T::default()
    }

    pub fn is_plain(adjustments: &Adjustments) -> bool {
        *adjustments == Adjustments::default()
    }

    pub fn serialize<S: Serializer>(adjustments: &Adjustments, to: S) -> Result<S::Ok, S::Error> {
        let name = |ratio: Option<Ratio>| ratio.map(|ratio| ratio.name.to_owned());
        Saved {
            turns: adjustments.orientation.clockwise_turns(),
            mirrored: adjustments.orientation.is_mirrored(),
            aspect_ratio: name(adjustments.aspect),
            crop: name(adjustments.crop),
        }
        .serialize(to)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Adjustments, D::Error> {
        let saved = Saved::deserialize(from)?;
        let named = |name: Option<String>, ratios: &[Ratio]| {
            let name = name?;
            ratios.iter().find(|ratio| ratio.name == name).copied()
        };
        Ok(Adjustments {
            orientation: Orientation::new(i32::from(saved.turns), saved.mirrored),
            aspect: named(saved.aspect_ratio, &ASPECT_RATIOS),
            crop: named(saved.crop, &CROPS),
        })
    }
}

/// Where a file was, and how it was set to play.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
struct Spot {
    /// Seconds into it, unless it starts over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<f64>,
    /// When it last played there, in seconds since 1970.
    played: u64,
    #[serde(flatten)]
    kept: Kept,
}

/// Where each file was, by its path.
#[derive(Default)]
pub struct Progress {
    files: HashMap<String, Spot>,
    /// It changed since it was saved.
    changed: bool,
}

impl Global for Progress {}

/// A file's place, as a block has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub file: PathBuf,
    /// Where it is, in seconds.
    pub position: f64,
    /// How long it is.
    pub duration: f64,
    pub kept: Kept,
}

/// The places' file, in the app's folder beside the settings.
const FILE: &str = "progress.json";

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
    /// The places saved in `folder`, but those too old.
    pub fn load(folder: &Folder) -> Self {
        let files = folder
            .file(FILE)
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

    fn note(&mut self, note: &Note, now: u64) {
        let key = key(&note.file);
        let position = worth_resuming(note.position, note.duration).then_some(note.position);
        if position.is_none() && note.kept.is_empty() {
            self.changed |= self.files.remove(&key).is_some();
            return;
        }
        // Left as it was: it didn't play since.
        if self
            .files
            .get(&key)
            .is_some_and(|spot| spot.position == position && spot.kept == note.kept)
        {
            return;
        }
        self.files.insert(
            key,
            Spot {
                position,
                played: now,
                kept: note.kept,
            },
        );
        self.changed = true;
    }

    fn spot(&self, file: &Path, now: u64) -> Option<&Spot> {
        self.files.get(&key(file)).filter(|spot| fresh(spot, now))
    }

    fn recall(&self, file: &Path, now: u64) -> Option<f64> {
        self.spot(file, now)?.position
    }

    fn kept(&self, file: &Path, now: u64) -> Kept {
        self.spot(file, now)
            .map(|spot| spot.kept)
            .unwrap_or_default()
    }

    fn forget_old(&mut self, now: u64) {
        let before = self.files.len();
        self.files.retain(|_, spot| fresh(spot, now));
        self.changed |= self.files.len() != before;
    }

    /// The places to save, if they changed since they were taken last.
    fn take_unsaved(&mut self) -> Option<Vec<u8>> {
        if !self.changed {
            return None;
        }
        self.forget_old(now());
        self.changed = false;
        serde_json::to_vec(&self.files)
            .inspect_err(|err| log::warn!("could not save the places: {err}"))
            .ok()
    }
}

/// Note where files are, as Settings say to, and save the places if
/// `save`: soon, off the UI thread.
pub fn note(notes: impl IntoIterator<Item = Note>, save: bool, cx: &mut App) {
    if !cx.global::<Settings>().resume_files {
        return;
    }
    let progress = cx.default_global::<Progress>();
    let now = now();
    for note in notes {
        progress.note(&note, now);
    }
    if save
        && let Some(contents) = progress.take_unsaved()
        && let Some(path) = Folder::of(cx).file(FILE)
    {
        crate::files::write_soon(path, contents, cx);
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

/// How `file` was set to play, if Settings say to resume files and it
/// played within `KEEP_DAYS`.
pub fn kept(file: &Path, cx: &App) -> Kept {
    if !cx.global::<Settings>().resume_files {
        return Kept::default();
    }
    cx.try_global::<Progress>()
        .map(|progress| progress.kept(file, now()))
        .unwrap_or_default()
}

/// Forget every file's place, the saved ones too, as Settings stop keeping
/// them.
pub fn forget_all(cx: &mut App) {
    cx.set_global(Progress::default());
    if let Some(path) = Folder::of(cx).file(FILE) {
        crate::files::remove(&path, cx).ok();
    }
}

#[cfg(test)]
#[path = "tests/progress.rs"]
mod tests;
