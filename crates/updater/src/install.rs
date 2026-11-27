//! Where the app is installed, found from its running executable: the app
//! bundle on macOS, the executable itself elsewhere (or the AppImage it
//! runs from); and swapping an update in for it, by rename, beside it.
//!
//! Not where the app isn't its own to update: built in a Cargo `target`
//! folder, installed by a package manager or store, or run by macOS from
//! a copy of its own until it's moved.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::package::remove;
use crate::{Error, NotInstalled, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    path: PathBuf,
}

/// The platforms installs are found for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
}

impl Os {
    pub const HERE: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else if cfg!(windows) {
        Self::Windows
    } else {
        Self::Linux
    };
}

impl Install {
    /// Where the running app is installed.
    pub fn here() -> Result<Self> {
        let exe = std::env::current_exe().map_err(|_| NotInstalled::NoExecutable)?;
        Ok(Self::find(Os::HERE, &exe, &|name| std::env::var_os(name))?)
    }

    /// Where the app whose executable is `exe` is installed, on `os`, its
    /// environment's variables read with `env`.
    pub fn find(
        os: Os,
        exe: &Path,
        env: &dyn Fn(&str) -> Option<OsString>,
    ) -> std::result::Result<Self, NotInstalled> {
        if built_from_source(exe) {
            return Err(NotInstalled::BuiltFromSource);
        }
        let text = exe.to_string_lossy();
        let path = match os {
            Os::Mac => {
                if text.contains("/AppTranslocation/") {
                    return Err(NotInstalled::Translocated);
                }
                bundle(exe).ok_or(NotInstalled::NotAnApp)?
            }
            Os::Linux => {
                if env("FLATPAK_ID").is_some() {
                    return Err(NotInstalled::Managed("Flatpak"));
                }
                if env("SNAP").is_some() {
                    return Err(NotInstalled::Managed("Snap"));
                }
                if let Some(appimage) = env("APPIMAGE") {
                    PathBuf::from(appimage)
                } else if ["/usr/", "/nix/", "/gnu/"]
                    .iter()
                    .any(|root| text.starts_with(root))
                {
                    return Err(NotInstalled::Managed("a package manager"));
                } else {
                    exe.to_owned()
                }
            }
            Os::Windows => {
                if text.contains("\\WindowsApps\\") || text.contains("/WindowsApps/") {
                    return Err(NotInstalled::Managed("the Microsoft Store"));
                }
                exe.to_owned()
            }
        };
        Ok(Self { path })
    }

    /// The install at `path`, as it is: an app bundle, or an executable.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where an update is put together, beside the install, so it's on
    /// the same volume and swaps in by rename.
    pub fn staging(&self) -> PathBuf {
        self.beside(".update")
    }

    /// Where the install goes as an update swaps in, until it's removed.
    fn old(&self) -> PathBuf {
        self.beside(".old")
    }

    /// `.<name><suffix>`, in the install's folder.
    fn beside(&self, suffix: &str) -> PathBuf {
        let mut name = OsString::from(".");
        name.push(self.path.file_name().unwrap_or_default());
        name.push(suffix);
        self.path.with_file_name(name)
    }

    /// Leftovers of an update gone: one put together and never swapped
    /// in, and the install it replaced, which Windows keeps while it runs.
    pub fn clean(&self) {
        remove(&self.staging());
        remove(&self.old());
    }

    /// That an update can be put together beside the install: its folder
    /// can be written to. The file it writes there is a new one, under a
    /// name of its own, as anyone who can write there too could have put
    /// a link where it goes: a new file is never one a link points to.
    pub(crate) fn check_writable(&self) -> Result<()> {
        self.check_writable_as(probe_name)
    }

    /// `check_writable`, its file named `.<name><suffix>` with each suffix
    /// `name` gives, till one isn't taken, a few times at most.
    fn check_writable_as(&self, mut name: impl FnMut() -> String) -> Result<()> {
        let folder = self.path.parent().unwrap_or(Path::new("."));
        let mut tries = 0;
        let made = loop {
            let probe = self.beside(&name());
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)
            {
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists && tries < 3 => tries += 1,
                made => break made.map(|_| probe),
            }
        };
        match made {
            Ok(probe) => {
                let _ = fs::remove_file(&probe);
                Ok(())
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
                ) =>
            {
                Err(Error::Permission(folder.to_owned()))
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Swap the update at `staged` in. The install is replaced at once
    /// where the system can (macOS's `renamex_np`, a rename over a file on
    /// Linux), else moved aside first and put back if the update can't
    /// take its place. On Windows a running executable can be renamed but
    /// not removed, so the old one stays aside until `clean`.
    pub fn swap(&self, staged: &Path) -> Result<()> {
        if fs::symlink_metadata(staged).is_err() {
            return Err(Error::update("nothing is staged"));
        }
        #[cfg(target_os = "macos")]
        match exchange(staged, &self.path) {
            Ok(()) => {
                // Where the update was is now the install it replaced.
                remove(staged);
                return Ok(());
            }
            Err(err) => log::info!(
                "can't swap {} in at once ({err}), so moving it aside",
                self.path.display()
            ),
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        if self.path.is_file() {
            fs::rename(staged, &self.path)?;
            return Ok(());
        }
        let old = self.old();
        remove(&old);
        fs::rename(&self.path, &old)?;
        if let Err(err) = fs::rename(staged, &self.path) {
            let _ = fs::rename(&old, &self.path);
            return Err(err.into());
        }
        // Windows can't remove an executable while it runs.
        #[cfg(not(windows))]
        remove(&old);
        Ok(())
    }
}

/// The app bundle `exe` is in: `<name>.app/Contents/MacOS/<exe>`.
fn bundle(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let named = |path: &Path, name: &str| path.file_name().is_some_and(|it| it == name);
    (named(macos, "MacOS")
        && named(contents, "Contents")
        && app.extension().is_some_and(|extension| extension == "app"))
    .then(|| app.to_owned())
}

/// Whether `exe` is in a Cargo `target` folder: one beside a
/// `Cargo.toml`.
fn built_from_source(exe: &Path) -> bool {
    exe.ancestors().any(|folder| {
        folder.file_name().is_some_and(|name| name == "target")
            && folder
                .parent()
                .is_some_and(|project| project.join("Cargo.toml").is_file())
    })
}

/// `a` and `b` swapped, at once.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn exchange(a: &Path, b: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let a = CString::new(a.as_os_str().as_bytes())?;
    let b = CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: both are NUL-terminated paths, alive through the call, which
    // only reads them.
    let swapped = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
    if swapped == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
#[path = "tests/install.rs"]
mod tests;

/// A name for `check_writable`'s file, this process's, this moment's and
/// this call's.
fn probe_name() -> String {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let made = MADE.fetch_add(1, Ordering::Relaxed);
    format!(".probe-{}-{now}-{made}", std::process::id())
}
