//! Updates from the feed `resources/updates.json` names, for where Plays
//! is installed.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use parking_lot::Mutex;
use serde::Deserialize;
use updater::{Config, Install, Keys, Staged, Updater, Url};

use super::{Found, Source};
use crate::ui::sentence::capitalized;

/// What `resources/updates.json` says.
#[derive(Deserialize)]
struct Settings {
    /// The feed's URL; none for a Plays that doesn't update itself.
    feed: Option<String>,
    /// The public keys the feed may be signed with, as minisign writes
    /// them.
    #[serde(default)]
    keys: Vec<String>,
}

const SETTINGS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/updates.json"
));

struct Feed {
    updater: Updater,
    install: Install,
    /// What the last check found.
    found: Mutex<Option<updater::Found>>,
    /// What was downloaded and put together, to swap in.
    staged: Mutex<Option<Staged>>,
}

/// The feed's updates, or why Plays doesn't update itself here; the
/// leftovers of an earlier update cleaned up.
pub(super) fn open() -> Result<Arc<dyn Source>, String> {
    let settings: Settings = serde_json::from_str(SETTINGS)
        .map_err(|err| format!("resources/updates.json isn't as it should be: {err}"))?;
    let Some(feed) = settings.feed else {
        return Err("this build of Plays doesn't update itself".into());
    };
    let feed = Url::parse(&feed).map_err(|err| format!("the feed's URL isn't one: {err}"))?;
    let keys =
        Keys::parse(settings.keys.iter().map(String::as_str)).map_err(|err| err.to_string())?;
    let install = Install::here().map_err(|err| capitalized(&err.to_string()))?;
    let downloads = dirs::cache_dir()
        .ok_or("there's no folder to download updates to")?
        .join("plays")
        .join("updates");
    let config = Config {
        app: "plays".into(),
        version: super::version(),
        feed,
        keys,
        platform: updater::platform(),
        downloads,
    };
    let leftovers = install.clone();
    std::thread::spawn(move || leftovers.clean());
    Ok(Arc::new(Feed {
        updater: Updater::new(config),
        install,
        found: Mutex::new(None),
        staged: Mutex::new(None),
    }))
}

fn cant(err: updater::Error) -> String {
    format!("can't update: {err}")
}

impl Source for Feed {
    fn check(&self) -> Result<Option<Found>, String> {
        let found = self.updater.check().map_err(cant)?;
        let said = found.as_ref().map(|found| Found {
            version: found.version.clone(),
            releases: found.releases.clone(),
            size: found.size(),
        });
        *self.found.lock() = found;
        Ok(said)
    }

    fn download(
        &self,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        let found = self
            .found
            .lock()
            .clone()
            .ok_or("nothing was found to download")?;
        let staged = self
            .updater
            .download(
                &found,
                &self.install,
                &mut |came| progress(came.done, came.total),
                cancel,
            )
            .map_err(cant)?;
        *self.staged.lock() = Some(staged);
        Ok(())
    }

    fn install(&self) -> Result<(), String> {
        let staged = self
            .staged
            .lock()
            .take()
            .ok_or("nothing is ready to install")?;
        staged.install().map_err(cant)
    }

    fn installed_at(&self) -> PathBuf {
        self.install.path().to_owned()
    }
}
