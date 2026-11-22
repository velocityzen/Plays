//! The app's own files, settings and where files were left: the folder
//! they are kept in, and writing them.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use gpui::{App, Global};

/// The folder the app keeps its files in: `plays` in the system's config
/// folder. Tests and smoke runs have none, so they never read or write the
/// real files, unless a test gives a folder of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folder(Option<PathBuf>);

impl Global for Folder {}

impl Folder {
    /// The app's: none in tests and smoke runs.
    pub fn app() -> Self {
        if cfg!(any(test, feature = "smoke")) {
            return Self(None);
        }
        Self(dirs::config_dir().map(|dir| dir.join("plays")))
    }

    /// An empty folder of a test's own, named for it.
    #[cfg(test)]
    pub fn for_test(name: &str) -> Self {
        let dir = std::env::temp_dir()
            .join("plays-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Self(Some(dir))
    }

    /// The folder set for the app, or else the app's.
    pub fn of(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_else(Self::app)
    }

    /// Where the file `name` is kept.
    pub fn file(&self, name: &str) -> Option<PathBuf> {
        self.0.as_ref().map(|dir| dir.join(name))
    }
}

/// Write `contents` to `path`, making its folder if need be. It goes to a
/// file beside it first, on the disk before it takes the name, so a crash
/// or losing power midway leaves the file as it was rather than half
/// written.
pub fn write_whole(path: &Path, contents: &[u8]) -> io::Result<()> {
    write(path, contents, false)
}

/// Write `contents` to `path` as `write_whole` does, the file readable and
/// writable by its owner alone: for secrets, such as what pairing with a
/// TV leaves.
pub fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    write(path, contents, true)
}

fn write(path: &Path, contents: &[u8], private: bool) -> io::Result<()> {
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty());
    if let Some(folder) = folder {
        fs::create_dir_all(folder)?;
    }
    let beside = with_suffix(path, ".tmp");
    // One left by a crash keeps the permissions it was made with.
    let _ = fs::remove_file(&beside);
    let written = create(&beside, private).and_then(|mut file| {
        file.write_all(contents)?;
        file.sync_all()
    });
    if let Err(err) = written.and_then(|()| fs::rename(&beside, path)) {
        let _ = fs::remove_file(&beside);
        return Err(err);
    }
    // The new name on the disk too; not every system can say so.
    if let Some(folder) = folder {
        let _ = File::open(folder).and_then(|folder| folder.sync_all());
    }
    Ok(())
}

/// A new file at `path`, for its owner alone if `private`.
#[cfg_attr(not(unix), allow(unused_variables))]
fn create(path: &Path, private: bool) -> io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// `path` with `suffix` after its whole name: `settings.json.bad`.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// A file's name, or its whole path without one.
pub fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// `path` as it's shown, the home folder shortened to `~`.
pub fn shown(path: &Path) -> String {
    shown_from(path, dirs::home_dir().as_deref())
}

fn shown_from(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest == Path::new("") => "~".to_owned(),
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn a_path_shows_the_home_folder_as_a_tilde() {
        let home = Some(Path::new("/home/me"));
        assert_eq!(
            shown_from(Path::new("/home/me/Movies/a.mkv"), home),
            "~/Movies/a.mkv"
        );
        assert_eq!(shown_from(Path::new("/home/me"), home), "~");
        assert_eq!(
            shown_from(Path::new("/home/meg/a.mkv"), home),
            "/home/meg/a.mkv"
        );
        assert_eq!(shown_from(Path::new("/media/a.mkv"), None), "/media/a.mkv");
    }

    #[test]
    fn a_file_is_replaced_whole_and_never_written_over() {
        let folder = std::env::temp_dir()
            .join("plays-tests")
            .join(format!("files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let path = folder.join("settings.json");
        write_whole(&path, b"{ \"old\": true }").unwrap();
        // What was there, as a reader that opened it before sees it.
        let mut before = fs::File::open(&path).unwrap();
        write_whole(&path, b"{ \"new\": true }").unwrap();
        let mut seen = String::new();
        before.read_to_string(&mut seen).unwrap();
        assert_eq!(seen, "{ \"old\": true }", "written over where it was");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ \"new\": true }");
        let left: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["settings.json"], "nothing else left beside it");
        let _ = fs::remove_dir_all(&folder);
    }

    #[cfg(unix)]
    #[test]
    fn a_private_file_is_its_owners_alone_though_one_left_beside_it_was_not() {
        use std::os::unix::fs::PermissionsExt;

        let folder = std::env::temp_dir()
            .join("plays-tests")
            .join(format!("private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let mode = |name: &str| {
            fs::metadata(folder.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        // Left by a crash, readable by all.
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("airplay.json.tmp"), b"").unwrap();
        fs::set_permissions(
            folder.join("airplay.json.tmp"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        write_private(&folder.join("airplay.json"), b"{}").unwrap();
        assert_eq!(mode("airplay.json"), 0o600);
        assert_eq!(fs::read(folder.join("airplay.json")).unwrap(), b"{}");
        let _ = fs::remove_dir_all(&folder);
    }
}
