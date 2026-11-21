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
mod tests {
    use airplay::PinPairing;
    use airplay::fake::{FakeReceiver, Kind};

    use super::*;

    /// Credentials from pairing with a fake TV.
    fn credentials() -> Credentials {
        let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
        PinPairing::start(&tv.device())
            .and_then(|pairing| pairing.finish("1234"))
            .expect("paired with the fake")
    }

    #[test]
    fn pairings_are_saved_in_the_apps_folder_and_read_back() {
        let folder = Folder::for_test("airplay-pairings");
        let credentials = credentials();
        let pairings = Pairings::load(&folder);
        let sender = pairings.sender();
        assert_eq!(sender.name, "Plays");
        pairings.keep("TV2", "Bedroom", &credentials);
        pairings.keep("TV1", "living room", &credentials);

        let again = Pairings::load(&folder);
        assert_eq!(again.sender().id, sender.id, "known again by the same id");
        assert_eq!(again.credentials("TV1"), Some(credentials.clone()));
        assert_eq!(again.credentials("TV3"), None);
        assert_eq!(
            again.paired(),
            [
                ("TV2".to_owned(), "Bedroom".to_owned()),
                ("TV1".to_owned(), "living room".to_owned())
            ],
            "by name"
        );

        again.forget("TV2");
        let after = Pairings::load(&folder);
        assert_eq!(after.credentials("TV2"), None);
        assert_eq!(after.paired().len(), 1);
    }

    #[test]
    fn a_broken_file_is_no_pairings_and_is_saved_over() {
        let folder = Folder::for_test("airplay-pairings-broken");
        let path = folder.file(FILE).unwrap();
        files::write_whole(&path, b"{ \"receivers\": ").unwrap();
        let pairings = Pairings::load(&folder);
        assert!(pairings.paired().is_empty());
        pairings.keep("TV1", "Den", &credentials());
        assert_eq!(Pairings::load(&folder).paired().len(), 1);
    }

    #[test]
    fn without_a_folder_they_are_kept_while_the_app_runs() {
        let pairings = Pairings::load(&Folder::default());
        let id = pairings.sender().id;
        assert_eq!(pairings.sender().id, id);
        pairings.keep("TV1", "Den", &credentials());
        assert!(pairings.credentials("TV1").is_some());
    }
}
