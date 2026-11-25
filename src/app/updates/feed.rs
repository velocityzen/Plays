//! Updates from the feed `resources/updates.json` names, for where Plays
//! is installed; or, in a debug build, from a try of updating
//! (`script/try-updates`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;
use updater::{Config, Install, Keys, Staged, Updater, Url};

use super::{FIRST_CHECK, Found, Source};
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

/// The folder of a try of updating, `script/try-updates`'s, in a debug
/// build.
#[cfg(debug_assertions)]
const TRYING: &str = "PLAYS_TRY_UPDATES";

/// A try of updating, as its folder's `try.json` has it: a feed on this
/// machine, signed with a key of its own, updating a stand-in for the
/// install and downloading to a folder of its own, so Plays' own install
/// and downloads are left alone.
#[cfg(debug_assertions)]
#[derive(Deserialize)]
struct Trying {
    feed: String,
    keys: Vec<String>,
    /// The stand-in updated in the install's place.
    install: PathBuf,
    downloads: PathBuf,
    /// How many seconds after starting it first checks.
    first_check: u64,
}

struct Feed {
    updater: Updater,
    install: Install,
    first_check: Duration,
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
    let (feed, keys) = parsed(&feed, &settings.keys)?;
    let install = Install::here().map_err(|err| capitalized(&err.to_string()))?;
    let downloads = dirs::cache_dir()
        .ok_or("there's no folder to download updates to")?
        .join("plays")
        .join("updates");
    Ok(made(feed, keys, install, downloads, FIRST_CHECK))
}

/// In a debug build, updates as the try of updating in the folder
/// `PLAYS_TRY_UPDATES` names has them, if it names one.
#[cfg(debug_assertions)]
pub(super) fn trying() -> Option<Result<Arc<dyn Source>, String>> {
    let folder = PathBuf::from(std::env::var_os(TRYING)?);
    Some(tried(&folder))
}

#[cfg(debug_assertions)]
fn tried(folder: &std::path::Path) -> Result<Arc<dyn Source>, String> {
    let path = folder.join("try.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("can't read {}: {err}", path.display()))?;
    let trying: Trying = serde_json::from_str(&text)
        .map_err(|err| format!("{} isn't as it should be: {err}", path.display()))?;
    let (feed, keys) = parsed(&trying.feed, &trying.keys)?;
    log::info!(
        "trying updates from {feed}, on {}",
        trying.install.display()
    );
    Ok(made(
        feed,
        keys,
        Install::at(trying.install),
        trying.downloads,
        Duration::from_secs(trying.first_check),
    ))
}

/// In a try of updating, Plays started again as this build, as it was
/// started, once it has quit: the try goes on. (macOS opens a build that
/// isn't an app bundle again in Terminal, with the try left out.)
#[cfg(debug_assertions)]
pub(super) fn run_again(cx: &mut gpui::App) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;

        let script = r#"while kill -0 "$0" 2>/dev/null; do sleep 0.1; done; exec "$@""#;
        let started = std::env::current_exe().and_then(|exe| {
            std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .arg(std::process::id().to_string())
                .arg(exe)
                .args(std::env::args_os().skip(1))
                .process_group(0)
                .spawn()
        });
        match started {
            Ok(_) => cx.quit(),
            Err(err) => log::error!("can't start Plays again: {err}"),
        }
    }
    #[cfg(not(unix))]
    cx.restart();
}

/// The feed's URL and its keys, as they're written.
fn parsed(feed: &str, keys: &[String]) -> Result<(Url, Keys), String> {
    let feed = Url::parse(feed).map_err(|err| format!("the feed's URL isn't one: {err}"))?;
    let keys = Keys::parse(keys.iter().map(String::as_str)).map_err(|err| err.to_string())?;
    Ok((feed, keys))
}

fn made(
    feed: Url,
    keys: Keys,
    install: Install,
    downloads: PathBuf,
    first_check: Duration,
) -> Arc<dyn Source> {
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
    Arc::new(Feed {
        updater: Updater::new(config),
        install,
        first_check,
        found: Mutex::new(None),
        staged: Mutex::new(None),
    })
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

    fn first_check(&self) -> Duration {
        self.first_check
    }
}
