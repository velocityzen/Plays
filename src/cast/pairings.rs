//! What pairing with AirPlay receivers left, kept in the app's folder
//! (`airplay.json`, its owner's alone): each receiver's credentials, by its
//! id, with its name for Settings; and the id Plays goes by, which
//! receivers know it again by.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use airplay::{Credentials, Sender};
use gpui::Global;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::files::{self, Folder};

const FILE: &str = "airplay.json";

/// What receivers are told the sender is called.
const SENDER: &str = "Plays";

/// A receiver paired with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Paired {
    name: String,
    /// As `Credentials::to_text` gives them.
    credentials: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Kept {
    /// The id Plays goes by, made up the first time it's asked for.
    sender: Option<String>,
    /// By receiver id.
    receivers: BTreeMap<String, Paired>,
}

/// The pairings, shared by the AirPlay target, which connects with them,
/// and Settings, which can forget them. Without a folder, as in tests,
/// they're kept while the app runs.
pub struct Pairings {
    file: Option<PathBuf>,
    kept: Mutex<Kept>,
}

/// The app's pairings, for Settings.
#[derive(Clone)]
pub struct AppPairings(pub Arc<Pairings>);

impl Global for AppPairings {}

impl Pairings {
    /// Those kept in `folder`: none if there's no file, or it can't be
    /// read, in which case it's saved over once there's a pairing to keep.
    pub fn load(folder: &Folder) -> Self {
        let file = folder.file(FILE);
        let kept = file
            .as_ref()
            .and_then(|path| {
                let text = fs::read_to_string(path).ok()?;
                serde_json::from_str(&text)
                    .inspect_err(|err| log::warn!("ignoring {}: {err}", path.display()))
                    .ok()
            })
            .unwrap_or_default();
        Self {
            file,
            kept: Mutex::new(kept),
        }
    }

    /// Plays, as receivers know it: the same id each time.
    pub fn sender(&self) -> Sender {
        let mut kept = self.kept.lock();
        if let Some(id) = &kept.sender {
            return Sender {
                name: SENDER.into(),
                id: id.clone(),
            };
        }
        let sender = Sender::new(SENDER);
        kept.sender = Some(sender.id.clone());
        self.save(&kept);
        sender
    }

    /// What pairing with the receiver `id` left, if it was paired with.
    pub fn credentials(&self, id: &str) -> Option<Credentials> {
        let kept = self.kept.lock();
        let paired = kept.receivers.get(id)?;
        Credentials::from_text(&paired.credentials)
    }

    /// Keep what pairing with the receiver `id`, called `name`, left.
    pub fn keep(&self, id: &str, name: &str, credentials: &Credentials) {
        let mut kept = self.kept.lock();
        kept.receivers.insert(
            id.to_owned(),
            Paired {
                name: name.to_owned(),
                credentials: credentials.to_text(),
            },
        );
        self.save(&kept);
    }

    /// Forget the receiver `id`, which then asks for its PIN again.
    pub fn forget(&self, id: &str) {
        let mut kept = self.kept.lock();
        if kept.receivers.remove(id).is_some() {
            self.save(&kept);
        }
    }

    /// The receivers paired with, by name: id and name.
    pub fn paired(&self) -> Vec<(String, String)> {
        let kept = self.kept.lock();
        let mut paired: Vec<(String, String)> = kept
            .receivers
            .iter()
            .map(|(id, paired)| (id.clone(), paired.name.clone()))
            .collect();
        paired.sort_by_cached_key(|(id, name)| (name.to_lowercase(), id.clone()));
        paired
    }

    fn save(&self, kept: &Kept) {
        let Some(path) = &self.file else {
            return;
        };
        let result = serde_json::to_string_pretty(kept)
            .map_err(std::io::Error::from)
            .and_then(|json| files::write_private(path, json.as_bytes()));
        if let Err(err) = result {
            log::warn!("could not save {}: {err}", path.display());
        }
    }
}

#[cfg(test)]
#[path = "tests/pairings.rs"]
mod tests;
