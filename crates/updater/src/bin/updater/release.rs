//! The feed: a release put first in it, it signed again, and checked.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use updater::feed::{self, Download, File, Header, Release};
use updater::{Keys, Version, hash, package};

use crate::keys::{self, Key};
use crate::{Result, at, parse_version, whole_version};

const DAY: u64 = 86_400;

/// A release to put first in a feed.
pub(crate) struct Added<'a> {
    pub notes: &'a Path,
    /// Each platform's whole update, as `PLATFORM=FILE`.
    pub updates: &'a [String],
    /// Each platform's deltas, as `PLATFORM=FILE`.
    pub deltas: &'a [String],
    /// Where the update files are, if not beside the feed.
    pub url: Option<&'a str>,
    /// The app the feed is for, if it's new.
    pub app: Option<&'a str>,
}

impl Added<'_> {
    /// The release put first in the feed at `path`, which is made if it
    /// isn't there, and signed with the secret key at `key` for `days`.
    pub(crate) fn into(self, path: &Path, key: &Path, days: u64) -> Result<()> {
        let existing = path.exists().then(|| read_text(path)).transpose()?;
        let now = now();
        let release = self.release(now)?;
        let key = keys::read(key)?;
        let text = add(existing.as_deref(), &release, self.app, &key, now, days)?;
        write(path, &text)?;
        let platforms: Vec<&str> = release.downloads.keys().map(String::as_str).collect();
        println!(
            "{}: {} first, for {}, signed for {days} days",
            path.display(),
            release.version,
            platforms.join(", ")
        );
        Ok(())
    }

    /// The release the update files are of, published at `now`.
    pub(crate) fn release(&self, now: u64) -> Result<Release> {
        let mut version: Option<Version> = None;
        let mut downloads = BTreeMap::new();
        for update in self.updates {
            let (platform, path) = split(update)?;
            let of = whole_version(path)?;
            if version.as_ref().is_some_and(|version| *version != of) {
                return Err(format!("{} is {of}, and the others aren't", path.display()));
            }
            version = Some(of);
            let whole = self.file(path)?;
            let download = Download {
                whole,
                deltas: BTreeMap::new(),
            };
            if downloads.insert(platform.to_owned(), download).is_some() {
                return Err(format!("{platform} has two updates"));
            }
        }
        let version = version.ok_or("a release needs an update")?;
        for delta in self.deltas {
            let (platform, path) = split(delta)?;
            let versions = package::versions(path).map_err(at(path))?;
            let from = versions
                .from
                .ok_or_else(|| format!("{} is a whole update, not a delta", path.display()))?;
            let from = parse_version(path, &from)?;
            let to = parse_version(path, &versions.to)?;
            if to != version {
                return Err(format!(
                    "{} is a delta to {to}, not {version}",
                    path.display()
                ));
            }
            let download = downloads
                .get_mut(platform)
                .ok_or_else(|| format!("{platform} has a delta, and no whole update"))?;
            if download
                .deltas
                .insert(from.to_string(), self.file(path)?)
                .is_some()
            {
                return Err(format!("{platform} has two deltas from {from}"));
            }
        }
        let notes = fs::read_to_string(self.notes).map_err(at(self.notes))?;
        Ok(Release {
            version,
            published: now,
            notes: notes.trim().to_owned(),
            downloads,
        })
    }

    /// The update file at `path`, as the feed says it is.
    fn file(&self, path: &Path) -> Result<File> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} has no name", path.display()))?;
        let url = match self.url {
            Some(base) => format!("{}/{name}", base.trim_end_matches('/')),
            None => name.to_owned(),
        };
        Ok(File {
            url,
            size: fs::metadata(path).map_err(at(path))?.len(),
            sha256: hash::of_file(path).map_err(at(path))?,
        })
    }
}

/// `PLATFORM=FILE`, split.
fn split(argument: &str) -> Result<(&str, &Path)> {
    argument
        .split_once('=')
        .filter(|(platform, file)| !platform.is_empty() && !file.is_empty())
        .map(|(platform, file)| (platform, Path::new(file)))
        .ok_or_else(|| format!("{argument} isn't PLATFORM=FILE"))
}

/// The feed `existing`, if there's one, with `release` first, for `app`
/// if it's new: signed with `key` at `now`, for `days`.
pub(crate) fn add(
    existing: Option<&str>,
    release: &Release,
    app: Option<&str>,
    key: &Key,
    now: u64,
    days: u64,
) -> Result<String> {
    let (app, lines) = match existing {
        Some(text) => {
            let header = header(text)?;
            if let Some(app) = app.filter(|app| *app != header.app) {
                return Err(format!("the feed is {}'s, not {app}'s", header.app));
            }
            if release.version <= header.latest {
                return Err(format!(
                    "{} isn't newer than the feed's latest, {}",
                    release.version, header.latest
                ));
            }
            (header.app, feed::release_lines(text))
        }
        None => {
            let app = app.ok_or("a new feed needs --app, the app it's for")?;
            (app.to_owned(), Vec::new())
        }
    };
    let first = feed::release_line(release, lines.first().map(String::as_str));
    let lines: Vec<String> = std::iter::once(first).chain(lines).collect();
    signed(&app, &lines, key, now, days)
}

/// The feed `text` signed again with `key` at `now`, for `days`, its
/// releases as they are.
pub(crate) fn again(text: &str, key: &Key, now: u64, days: u64) -> Result<String> {
    let header = header(text)?;
    let lines = feed::release_lines(text);
    if lines.is_empty() {
        return Err("the feed has no releases".into());
    }
    signed(&header.app, &lines, key, now, days)
}

/// The feed of `lines`, signed, once it reads back as `app`'s.
fn signed(app: &str, lines: &[String], key: &Key, now: u64, days: u64) -> Result<String> {
    let expires = now + days.max(1) * DAY;
    let header = feed::header_line(app, now, expires, &lines[0]).map_err(|err| err.to_string())?;
    let text = feed::join(&key.sign(&header)?, &header, lines);
    read(&text, &key.keys()?, now)?;
    Ok(text)
}

/// Every release in the feed `text`, which must hold, signed with one of
/// `keys`, at `now`.
fn read(text: &str, keys: &Keys, now: u64) -> Result<Vec<Release>> {
    let header = header(text)?;
    let oldest = Version::parse("0.0.0-0").expect("a version");
    let feed = feed::read(Cursor::new(text), keys, &header.app, &oldest, now)
        .map_err(|err| err.to_string())?;
    Ok(feed.map(|feed| feed.releases).unwrap_or_default())
}

/// The feed `text`'s header, unchecked.
fn header(text: &str) -> Result<Header> {
    let line = text.lines().nth(1).ok_or("the feed has no header")?;
    serde_json::from_str(line).map_err(|err| format!("the feed's header isn't one: {err}"))
}

/// Sign the feed at `path` again with the secret key at `key`, for `days`.
pub(crate) fn sign_again(path: &Path, key: &Path, days: u64) -> Result<()> {
    let text = read_text(path)?;
    let key = keys::read(key)?;
    write(path, &again(&text, &key, now(), days)?)?;
    println!("{}: signed for {days} days", path.display());
    Ok(())
}

/// Check the feed at `path` with the public key at `key`, and say what's
/// in it, if it's taken for `at_least` days more.
pub(crate) fn verify(path: &Path, keys: &[PathBuf], at_least: u64) -> Result<()> {
    let text = read_text(path)?;
    let publics = keys
        .iter()
        .map(|key| fs::read_to_string(key).map_err(at(key)))
        .collect::<Result<Vec<_>>>()?;
    let keys = Keys::parse(publics.iter().map(String::as_str)).map_err(|err| err.to_string())?;
    let now = now();
    let releases = read(&text, &keys, now).map_err(at(path))?;
    let header = header(&text)?;
    let left = days_left(&header, now, at_least).map_err(at(path))?;
    println!(
        "{}: {}'s, signed, taken for {left} days more",
        path.display(),
        header.app
    );
    for release in releases {
        let downloads: Vec<String> = release
            .downloads
            .iter()
            .map(|(platform, download)| said(platform, download))
            .collect();
        println!("{}: {}", release.version, downloads.join("; "));
    }
    Ok(())
}

/// The whole days the feed of `header` is taken for after `now`: at least
/// `at_least`, or it's to be signed again.
fn days_left(header: &Header, now: u64, at_least: u64) -> Result<u64> {
    let left = header.expires.saturating_sub(now) / DAY;
    if left < at_least {
        return Err(format!(
            "it's taken for {left} days more, fewer than {at_least}: sign it again"
        ));
    }
    Ok(left)
}

/// A platform's download, as `verify` says it.
fn said(platform: &str, download: &Download) -> String {
    if download.deltas.is_empty() {
        return platform.to_owned();
    }
    let from: Vec<&str> = download.deltas.keys().map(String::as_str).collect();
    format!("{platform}, deltas from {}", from.join(", "))
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(at(path))
}

/// `text` written to `path` whole, or not at all.
fn write(path: &Path, text: &str) -> Result<()> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    fs::write(&partial, text).map_err(at(path))?;
    fs::rename(&partial, path).map_err(at(path))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
#[path = "tests/release.rs"]
mod tests;
