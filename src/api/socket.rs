//! The socket: one Plays runs, and the command line, or another Plays
//! started with files, reaches it here. JSON lines go over it
//! (`protocol`): a client asks Plays to run a command, as a key would,
//! lists and describes the commands, and hears events (`api::events`).
//! Nothing goes on the network: it's a Unix socket, in a folder only this
//! user can change, that only they can connect to.
//!
//! Plays takes it as it starts (`start`), so one runs: one already
//! running, another hands it the files it was given and exits; a socket
//! left by one that crashed is taken over. The app answers what comes over
//! it (`server`), with control from the command line off in Settings only
//! files handed over; `client` is the other end.

pub mod client;
mod protocol;
mod server;

use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

pub use protocol::Heard;
pub use server::{NotListening, serve};

/// The socket's name, in the folder `path` finds.
const NAME: &str = "plays.sock";

/// What `claim` holds while it looks, so two Plays starting at once don't
/// both take the socket.
const LOCK: &str = "plays.lock";

/// How many times `claim` tries, another Plays taking the socket as it
/// looks.
const TRIES: usize = 3;

/// The environment variable naming a socket of Plays' own, where `path`
/// would find another: for a second Plays running apart, a smoke run's,
/// and the command line talking to either.
pub const VARIABLE: &str = "PLAYS_SOCKET";

/// Where the socket is: where `PLAYS_SOCKET` says, else in the app's
/// folder on macOS, in `$XDG_RUNTIME_DIR/plays` on Linux (the app's folder
/// if there's none). None in tests and smoke runs, unless named, as they
/// run alone, with no settings folder either.
pub fn path() -> Option<PathBuf> {
    path_named(std::env::var_os(VARIABLE))
}

/// Where the socket is, `named` there or not.
fn path_named(named: Option<OsString>) -> Option<PathBuf> {
    if let Some(named) = named.filter(|named| !named.is_empty()) {
        return Some(named.into());
    }
    if cfg!(any(test, feature = "smoke")) {
        return None;
    }
    #[cfg(target_os = "linux")]
    if let Some(run) = dirs::runtime_dir() {
        return Some(run.join("plays").join(NAME));
    }
    dirs::config_dir().map(|dir| dir.join("plays").join(NAME))
}

/// The socket, taken: what `serve` answers on.
pub struct Listener {
    listener: UnixListener,
}

/// What `claim` found.
pub enum Claimed {
    /// No Plays answered there: this one listens now.
    Ours(Listener),
    /// Another Plays answers there.
    Running,
}

/// Take the socket at `path`, unless a Plays answers there: one that
/// doesn't, which crashed, left it, and it's taken over. Its folder is made
/// if it isn't there, open to this user only, and one there already must
/// be theirs; the socket is theirs only.
pub fn claim(path: &Path) -> io::Result<Claimed> {
    made_private(folder(path))?;
    let _looking = locked(path)?;
    for _ in 0..TRIES {
        match UnixStream::connect(path) {
            Ok(_) => return Ok(Claimed::Running),
            // Left by a Plays that's gone.
            Err(err) if err.kind() == io::ErrorKind::ConnectionRefused => {
                log::info!("taking over the socket a Plays left: {}", path.display());
                remove(path)?;
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        match UnixListener::bind(path) {
            Ok(listener) => {
                fs::set_permissions(path, Permissions::from_mode(0o600))?;
                return Ok(Claimed::Ours(Listener { listener }));
            }
            Err(err) if err.kind() == io::ErrorKind::AddrInUse => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::other("another Plays keeps taking the socket"))
}

/// The folder the socket at `path` is in.
fn folder(path: &Path) -> &Path {
    path.parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Make `folder`, if it isn't there, open to this user only. One there
/// already must be theirs, and no one else may write in it: else another
/// could put what they like where the socket goes. Others may look in it,
/// as in the app's folder, which holds the socket on macOS.
fn made_private(folder: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)?;
    let found = fs::metadata(folder)?;
    // SAFETY: geteuid has no preconditions and can't fail.
    let theirs = found.uid() == unsafe { libc::geteuid() };
    if !theirs || found.permissions().mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} isn't this user's alone", folder.display()),
        ));
    }
    Ok(())
}

/// The lock beside the socket at `path`, taken: while it's held, no other
/// Plays takes or removes the socket. Opened as it is, never through a
/// link, nor emptied.
fn locked(path: &Path) -> io::Result<File> {
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(folder(path).join(LOCK))?;
    lock.lock()?;
    Ok(lock)
}

/// Remove the socket at `path`, if it's there; anything else there stays.
fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(found) if found.file_type().is_socket() => fs::remove_file(path),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} isn't a socket", path.display()),
        )),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// How Plays starts.
pub enum Start {
    /// This one runs: listening on the socket, or without one if it has
    /// none (tests, smoke runs) or can't take it.
    Runs(Option<Listener>),
    /// Another runs, and was handed what this one was given: this one exits
    /// with the status.
    HandedOver(u8),
}

/// Take the socket, or, if another Plays runs, hand it `args` (files,
/// folders and URLs, as given on the command line): it opens them, or with
/// none comes to the front.
pub fn start(args: &[OsString]) -> Start {
    start_at(path(), args)
}

/// `start`, with the socket at `path`, if there's one.
fn start_at(path: Option<PathBuf>, args: &[OsString]) -> Start {
    let Some(path) = path else {
        return Start::Runs(None);
    };
    match claim(&path) {
        Ok(Claimed::Ours(listener)) => Start::Runs(Some(listener)),
        Ok(Claimed::Running) => match client::hand_over(&path, args) {
            Ok(()) => Start::HandedOver(0),
            Err(err) => {
                eprintln!("plays: {err}");
                Start::HandedOver(err.exit_status())
            }
        },
        Err(err) => {
            log::warn!("running without a socket, {}: {err}", path.display());
            Start::Runs(None)
        }
    }
}

#[cfg(test)]
#[path = "tests/socket.rs"]
pub(crate) mod tests;
