//! An app updating itself, on macOS, Linux and Windows. Knows nothing of
//! the app beyond its name, version, feed and keys.
//!
//! - `feed`: the releases, newest first, read as they come down to the
//!   running version; each line signed with minisign on its own
//! - `fetch`: HTTPS in bounds; downloads hashed as they come
//! - `package`: update files, whole or a delta from a version, file by
//!   file, each checked as it's written
//! - `install`: where the app is installed, and swapping an update in
//!
//! [`Updater::check`] finds an update; [`Updater::download`] downloads
//! it, the delta from the running version first if the feed has one, and
//! puts it together beside the install; [`Staged::install`] swaps it in,
//! for the app to run once it starts again.

#![deny(unsafe_code)]

mod error;
pub mod feed;
mod fetch;
pub mod hash;
mod install;
pub mod package;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

pub use error::{Error, NotInstalled, Result};
pub use feed::{Feed, Keys, Release, platform};
pub use install::{Install, Os};
pub use semver::Version;
pub use url::Url;

use feed::File;
use fetch::Client;

/// What an app checks for updates with.
#[derive(Clone, Debug)]
pub struct Config {
    /// The app's name, as its feed has it.
    pub app: String,
    /// The running version.
    pub version: Version,
    /// The feed's URL.
    pub feed: Url,
    pub keys: Keys,
    /// The platform's downloads to take, `platform()` unless a test says.
    pub platform: String,
    /// Where downloads are kept until they're installed.
    pub downloads: PathBuf,
}

pub struct Updater {
    config: Config,
    client: Client,
}

/// A newer version, which the feed has an update to for this platform.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub version: Version,
    /// Each release for this platform since the running version, newest
    /// first, with its notes in Markdown.
    pub releases: Vec<Release>,
    whole: (Url, File),
    /// The delta from the running version, if the feed has one.
    delta: Option<(Url, File)>,
}

impl Found {
    /// How many bytes it takes to download: the delta's, if there's one.
    pub fn size(&self) -> u64 {
        self.delta.as_ref().unwrap_or(&self.whole).1.size
    }

    /// Whether there's a delta to it from the running version.
    pub fn has_delta(&self) -> bool {
        self.delta.is_some()
    }
}

/// How far a download has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// An update put together beside the install, to swap in.
#[derive(Debug)]
pub struct Staged {
    install: Install,
    version: Version,
    downloads: PathBuf,
}

impl Updater {
    pub fn new(config: Config) -> Self {
        let client = Client::new(&format!("{}/{}", config.app, config.version));
        Self { config, client }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Read the feed, as far as the running version, and check it: the
    /// newer version it has, if any.
    pub fn check(&self) -> Result<Option<Found>> {
        let config = &self.config;
        let lines = self.client.open(&config.feed)?;
        let read = feed::read(lines, &config.keys, &config.app, &config.version, now());
        let feed = match read {
            Ok(Some(feed)) => feed,
            Ok(None) => return Ok(None),
            // What's read is what comes over the network.
            Err(Error::Io(err)) => {
                return Err(Error::Network {
                    url: config.feed.to_string(),
                    message: err.to_string(),
                });
            }
            Err(err) => return Err(err),
        };
        // A release may be for some platforms alone, as a fix for one OS
        // is: those for others are passed over.
        let releases: Vec<Release> = feed
            .releases
            .into_iter()
            .filter(|release| release.downloads.contains_key(&config.platform))
            .collect();
        let Some(newest) = releases.first() else {
            return Ok(None);
        };
        let download = &newest.downloads[&config.platform];
        let whole = (download.whole.url(&config.feed)?, download.whole.clone());
        let delta = download
            .deltas
            .get(&config.version.to_string())
            .map(|delta| Ok::<_, Error>((delta.url(&config.feed)?, delta.clone())))
            .transpose()?;
        Ok(Some(Found {
            version: newest.version.clone(),
            releases,
            whole,
            delta,
        }))
    }

    /// Download `found`'s update and put it together beside `install`:
    /// the delta from the running version first, if there's one, and the
    /// whole update if anything about the delta fails. `progress` is told
    /// how far each download has come.
    pub fn download(
        &self,
        found: &Found,
        install: &Install,
        progress: &mut dyn FnMut(Progress),
        cancel: &AtomicBool,
    ) -> Result<Staged> {
        install.check_writable()?;
        fs::create_dir_all(&self.config.downloads)?;
        install.clean();
        if let Some(delta) = &found.delta {
            match self.fetch_and_apply(found, delta, true, install, progress, cancel) {
                Ok(()) => return self.staged(found, install),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(err) => log::warn!(
                    "the delta to {} didn't apply, so downloading it whole: {err}",
                    found.version
                ),
            }
        }
        self.fetch_and_apply(found, &found.whole, false, install, progress, cancel)?;
        self.staged(found, install)
    }

    /// Download `file` from `url` and apply it beside `install`: as a
    /// delta from it, if `delta`.
    fn fetch_and_apply(
        &self,
        found: &Found,
        (url, file): &(Url, File),
        delta: bool,
        install: &Install,
        progress: &mut dyn FnMut(Progress),
        cancel: &AtomicBool,
    ) -> Result<()> {
        let config = &self.config;
        let name = if delta {
            format!(
                "{}-{}-{}.update",
                config.version, found.version, config.platform
            )
        } else {
            format!("{}-{}.update", found.version, config.platform)
        };
        let downloaded = config.downloads.join(name);
        let total = file.size;
        let mut told = |done| progress(Progress { done, total });
        self.client
            .download(url, file, &downloaded, &mut told, cancel)?;
        let staging = install.staging();
        package::remove(&staging);
        let base = delta.then(|| install.path());
        let applied = package::apply(&downloaded, base, &found.version.to_string(), &staging);
        if applied.is_err() {
            // As it is, it never will.
            package::remove(&downloaded);
        }
        applied
    }

    /// The update put together beside `install`, once its code signature
    /// holds.
    fn staged(&self, found: &Found, install: &Install) -> Result<Staged> {
        if let Err(err) = check_code_signature(install.path(), &install.staging()) {
            install.clean();
            return Err(err);
        }
        Ok(Staged {
            install: install.clone(),
            version: found.version.clone(),
            downloads: self.config.downloads.clone(),
        })
    }
}

/// macOS: that the app at `staged` passes `codesign --verify --strict`,
/// and is signed by the team that signed the one at `running`, if a team
/// did. Elsewhere the feed's signature is all there is.
#[cfg(target_os = "macos")]
fn check_code_signature(running: &Path, staged: &Path) -> Result<()> {
    use std::process::Command;

    if !staged.is_dir() {
        return Ok(());
    }
    let verified = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", "--deep"])
        .arg(staged)
        .output()?;
    if !verified.status.success() {
        let said = String::from_utf8_lossy(&verified.stderr);
        return Err(Error::CodeSignature(
            said.trim().replace(&*staged.to_string_lossy(), "it"),
        ));
    }
    let team = |app: &Path| -> Result<Option<String>> {
        let shown = Command::new("/usr/bin/codesign")
            .args(["--display", "--verbose=2"])
            .arg(app)
            .output()?;
        Ok(String::from_utf8_lossy(&shown.stderr)
            .lines()
            .find_map(|line| line.strip_prefix("TeamIdentifier="))
            .filter(|team| *team != "not set")
            .map(str::to_owned))
    };
    if let Some(running_team) = team(running)?
        && team(staged)?.as_ref() != Some(&running_team)
    {
        return Err(Error::CodeSignature(format!(
            "it isn't signed by team {running_team}"
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn check_code_signature(_: &Path, _: &Path) -> Result<()> {
    Ok(())
}

impl Staged {
    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn install_path(&self) -> &Path {
        self.install.path()
    }

    /// Swap it in for the install: the app runs it once it starts again,
    /// from the same place. The downloads it was made from go.
    pub fn install(self) -> Result<()> {
        self.install.swap(&self.install.staging())?;
        forget(&self.downloads);
        Ok(())
    }

    /// Throw it away.
    pub fn discard(self) {
        self.install.clean();
    }
}

/// The downloads in `folder` gone.
fn forget(folder: &Path) {
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        package::remove(&entry.path());
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
#[path = "tests/updater.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/support.rs"]
mod support;
