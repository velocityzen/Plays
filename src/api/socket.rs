//! The socket: one Plays runs, and the command line, or another Plays
//! started with files, reaches it here. JSON lines go over it
//! (`protocol`): a client asks Plays to run a command, as a key would,
//! lists and describes the commands, and hears events (`api::events`).
//! Nothing goes on the network: it's a Unix socket, in a folder only this
//! user can change, that only they can connect to, and only their
//! processes are answered on.
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
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// The socket's name in the folder it's bound in, aside (`Aside`): short,
/// as a socket's path has room for about a hundred bytes.
const BOUND: &str = "s";

/// How many names `Aside::made` tries, a process gone having left one.
const NAMES: usize = 4;

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
    /// The user whose processes it answers: this one.
    user: libc::uid_t,
}

impl Listener {
    /// Whether `client` is a process of the user it answers. Another's
    /// could connect only through a folder or socket opened to them.
    fn admits(&self, client: &UnixStream) -> bool {
        peer(client).is_ok_and(|user| user == self.user)
    }
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
/// be theirs; the socket is theirs only before it's there (`bound`).
pub fn claim(path: &Path) -> io::Result<Claimed> {
    made_private(folder(path))?;
    let _looking = locked(path)?;
    for _ in 0..TRIES {
        match UnixStream::connect(path) {
            Ok(answering) => {
                // So it hears this one leave, as a process started as it
                // connected may have its socket too.
                answering.shutdown(Shutdown::Both).ok();
                return Ok(Claimed::Running);
            }
            // Left by a Plays that's gone.
            Err(err) if err.kind() == io::ErrorKind::ConnectionRefused => {
                log::info!("taking over the socket a Plays left: {}", path.display());
                remove(path)?;
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        match bound(path) {
            Ok(listener) => {
                let user = this_user();
                return Ok(Claimed::Ours(Listener { listener, user }));
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::other("another Plays keeps taking the socket"))
}

/// A socket bound at `path`, this user's alone before another could reach
/// it: bound aside, made theirs only there, then linked into place, which
/// takes nothing there already (`AlreadyExists`).
fn bound(path: &Path) -> io::Result<UnixListener> {
    let aside = Aside::made(folder(path))?;
    let bound = aside.0.join(BOUND);
    let listener = UnixListener::bind(&bound)?;
    fs::set_permissions(&bound, Permissions::from_mode(0o600))?;
    fs::hard_link(&bound, path)?;
    Ok(listener)
}

/// A folder beside the socket that only this user can open, the socket
/// bound in it first. Removed, with the socket's name in it, as it's
/// dropped.
struct Aside(PathBuf);

impl Aside {
    /// Make one in `folder`, under a name of this process's and call's
    /// own.
    fn made(folder: &Path) -> io::Result<Self> {
        static MADE: AtomicU64 = AtomicU64::new(0);
        Self::made_as(folder, || {
            let made = MADE.fetch_add(1, Ordering::Relaxed);
            format!(".{}.{made}", std::process::id())
        })
    }

    /// Make one in `folder`, under the name `name` gives, another if it's
    /// taken, a few times at most.
    fn made_as(folder: &Path, mut name: impl FnMut() -> String) -> io::Result<Self> {
        for _ in 0..NAMES {
            let aside = folder.join(name());
            match DirBuilder::new().mode(0o700).create(&aside) {
                Ok(()) => return Ok(Self(aside)),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(err),
            }
        }
        Err(io::Error::other(format!(
            "no name free in {} to bind the socket under",
            folder.display()
        )))
    }
}

impl Drop for Aside {
    fn drop(&mut self) {
        fs::remove_file(self.0.join(BOUND)).ok();
        fs::remove_dir(&self.0).ok();
    }
}

/// The user this Plays runs as.
fn this_user() -> libc::uid_t {
    // SAFETY: geteuid has no preconditions and can't fail.
    unsafe { libc::geteuid() }
}

/// The user whose process is at the other end of `stream`, as it
/// connected.
#[cfg(not(target_os = "linux"))]
fn peer(stream: &UnixStream) -> io::Result<libc::uid_t> {
    let (mut user, mut group) = (0, 0);
    // SAFETY: the descriptor is the stream's, open while it's borrowed;
    // getpeereid writes only the two numbers.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut user, &mut group) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(user)
}

/// The user whose process is at the other end of `stream`, as it
/// connected.
#[cfg(target_os = "linux")]
fn peer(stream: &UnixStream) -> io::Result<libc::uid_t> {
    let mut found = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the descriptor is the stream's, open while it's borrowed;
    // getsockopt writes at most `length` bytes, a ucred's, into `found`.
    let got = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut found).cast(),
            &mut length,
        )
    };
    if got != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(found.uid)
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
    let theirs = found.uid() == this_user();
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
