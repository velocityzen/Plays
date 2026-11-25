//! `updater`, which makes a release: the key pair a feed is signed with,
//! update files for an install, whole and as deltas from earlier ones,
//! and the feed, signed. `cargo run -p updater --features release --`
//! runs it; `--help` says how.
//!
//! The secret key is encrypted with a password, asked for on the terminal,
//! or taken from `UPDATER_PASSWORD` where there's none.

#![forbid(unsafe_code)]

mod keys;
mod release;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use updater::{Version, package};

type Result<T> = std::result::Result<T, String>;

/// A zstd level that's slow to compress at, and as quick to decompress
/// as any.
const LEVEL: i32 = 19;
/// How long a feed is taken for, by default, before it must be signed
/// again: a feed held back longer is refused.
const DAYS: u64 = 30;

#[derive(Parser)]
#[command(
    name = "updater",
    about = "Makes a release's keys, update files and signed feed"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make the key pair feeds are signed with: FOLDER/updates.key, the
    /// secret key, encrypted with a password, and FOLDER/updates.pub.
    Keygen { folder: PathBuf },
    /// Make the update file OUT of the install at INSTALL (an app bundle
    /// or a binary), version VERSION, whole.
    Pack {
        install: PathBuf,
        #[arg(long)]
        version: Version,
        out: PathBuf,
    },
    /// Make the update file OUT, a delta from the version whole in FROM
    /// to the version whole in TO: update files `pack` made.
    Delta {
        from: PathBuf,
        to: PathBuf,
        out: PathBuf,
    },
    /// Put the version the update files are of first in FEED, made if
    /// there's none, and sign it with KEY.
    Feed {
        feed: PathBuf,
        /// What's new in it, in Markdown.
        #[arg(long)]
        notes: PathBuf,
        /// A platform's whole update, as PLATFORM=FILE, such as
        /// macos-aarch64=Plays-0.2.0-macos-aarch64.update.
        #[arg(long = "update", value_name = "PLATFORM=FILE", required = true)]
        updates: Vec<String>,
        /// A platform's delta to it, from an earlier version, as
        /// PLATFORM=FILE.
        #[arg(long = "delta", value_name = "PLATFORM=FILE")]
        deltas: Vec<String>,
        /// Where the update files are, if not beside the feed.
        #[arg(long)]
        url: Option<String>,
        /// The app the feed is for, for a new one.
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        key: PathBuf,
        /// How many days the feed is taken for.
        #[arg(long, default_value_t = DAYS)]
        days: u64,
    },
    /// Sign FEED's header again, to be taken for DAYS more. The releases'
    /// lines stay as they are, but those another key signed, which are
    /// signed with KEY.
    Sign {
        feed: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value_t = DAYS)]
        days: u64,
    },
    /// Check FEED is signed with one of the public keys KEY, and is taken
    /// for DAYS more at least, and say its releases.
    Verify {
        feed: PathBuf,
        /// A public key: a `.pub` file, or a file of its key line alone.
        #[arg(long = "key", required = true)]
        keys: Vec<PathBuf>,
        /// How many days more it must be taken for.
        #[arg(long, value_name = "DAYS", default_value_t = 0)]
        at_least: u64,
    },
}

fn main() -> ExitCode {
    let done = match Args::parse().command {
        Command::Keygen { folder } => keys::generate(&folder),
        Command::Pack {
            install,
            version,
            out,
        } => pack(&install, &version, &out),
        Command::Delta { from, to, out } => delta(&from, &to, &out),
        Command::Feed {
            feed,
            notes,
            updates,
            deltas,
            url,
            app,
            key,
            days,
        } => release::Added {
            notes: &notes,
            updates: &updates,
            deltas: &deltas,
            url: url.as_deref(),
            app: app.as_deref(),
        }
        .into(&feed, &key, days),
        Command::Sign { feed, key, days } => release::sign_again(&feed, &key, days),
        Command::Verify {
            feed,
            keys,
            at_least,
        } => release::verify(&feed, &keys, at_least),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("updater: {error}");
            ExitCode::FAILURE
        }
    }
}

fn pack(install: &Path, version: &Version, out: &Path) -> Result<()> {
    let made =
        package::make(install, None, &version.to_string(), LEVEL, out).map_err(at(install))?;
    println!(
        "{}: {version}, {}, {}",
        out.display(),
        files(made.whole),
        size(out)?
    );
    Ok(())
}

/// The delta from `from`'s version to `to`'s, each a whole update file:
/// both put together, then the one made from the other.
fn delta(from: &Path, to: &Path, out: &Path) -> Result<()> {
    let scratch = std::env::temp_dir().join(format!("updater-delta-{}", std::process::id()));
    let made = delta_in(from, to, out, &scratch);
    let _ = fs::remove_dir_all(&scratch);
    made
}

fn delta_in(from: &Path, to: &Path, out: &Path, scratch: &Path) -> Result<()> {
    let from_version = whole_version(from)?;
    let to_version = whole_version(to)?;
    if to_version <= from_version {
        return Err(format!(
            "{} is {to_version}, not newer than {from_version}",
            to.display()
        ));
    }
    fs::create_dir_all(scratch).map_err(at(scratch))?;
    let (old, new) = (scratch.join("from"), scratch.join("to"));
    package::apply(from, None, &from_version.to_string(), &old).map_err(at(from))?;
    package::apply(to, None, &to_version.to_string(), &new).map_err(at(to))?;
    let base = Some((old.as_path(), from_version.to_string()));
    let base = base
        .as_ref()
        .map(|(path, version)| (*path, version.as_str()));
    let made = package::make(&new, base, &to_version.to_string(), LEVEL, out).map_err(at(out))?;
    println!(
        "{}: {from_version} to {to_version}, {} kept, {} patched, {} whole, {}",
        out.display(),
        files(made.kept),
        made.patched,
        made.whole,
        size(out)?
    );
    Ok(())
}

/// The version the whole update file at `path` is of.
pub(crate) fn whole_version(path: &Path) -> Result<Version> {
    let versions = package::versions(path).map_err(at(path))?;
    if let Some(from) = versions.from {
        return Err(format!(
            "{} is a delta from {from}, not a whole update",
            path.display()
        ));
    }
    parse_version(path, &versions.to)
}

pub(crate) fn parse_version(path: &Path, version: &str) -> Result<Version> {
    Version::parse(version)
        .map_err(|err| format!("{}: {version} isn't a version: {err}", path.display()))
}

fn files(count: usize) -> String {
    if count == 1 {
        "1 file".into()
    } else {
        format!("{count} files")
    }
}

fn size(path: &Path) -> Result<String> {
    let bytes = fs::metadata(path).map_err(at(path))?.len() as f64;
    Ok(if bytes < 1e6 {
        format!("{:.1} KB", bytes / 1e3)
    } else {
        format!("{:.1} MB", bytes / 1e6)
    })
}

/// An error about `path`, saying so.
pub(crate) fn at<E: std::fmt::Display>(path: &Path) -> impl Fn(E) -> String + '_ {
    move |err| format!("{}: {err}", path.display())
}

#[cfg(test)]
#[path = "tests/main.rs"]
mod tests;
