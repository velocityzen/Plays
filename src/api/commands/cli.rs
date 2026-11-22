//! `cli`: the command line, put where a terminal finds it.

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::api::Error;
use crate::api::registry::{Outcome, Spec};

/// Where the command line goes: on the PATH a terminal has to start with.
const FOLDER: &str = "/usr/local/bin";

pub const COMMANDS: &[Spec] = &[Spec::app_with(
    "cli.install",
    "Install Command Line Tool",
    "Link plays into /usr/local/bin, so a terminal can run it: to open files, run \
     commands and hear what changes.",
    &[],
    |_, _| install(),
)
.keywords(&["terminal", "shell", "path", "command line"])
// Elsewhere, Plays is installed where the system keeps programs.
.only_if(|| cfg!(target_os = "macos"))
.returns("The link's path.")];

/// Link Plays, this program, into `FOLDER`.
fn install() -> Outcome {
    let program = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|err| Error::failed(format!("can't find Plays itself: {err}")))?;
    let link = link(&program, Path::new(FOLDER))?;
    Ok(Value::from(link.display().to_string()))
}

/// Link `program` into `folder` as `plays`, in place of a link there but
/// of nothing else; if `folder` can't be written to, say what to type in
/// a terminal instead.
fn link(program: &Path, folder: &Path) -> Result<PathBuf, Error> {
    let link = folder.join("plays");
    match fs::symlink_metadata(&link) {
        Ok(found) if !found.is_symlink() => {
            return Err(Error::failed(format!(
                "{} is there already, and isn't a link: move it away first",
                link.display()
            )));
        }
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(cant(program, &link, &err)),
    }
    // Made beside it and moved over it, so a link there stays until the
    // new one replaces it.
    let made = folder.join(format!(".plays-{}", std::process::id()));
    fs::remove_file(&made).ok();
    symlink(program, &made).map_err(|err| cant(program, &link, &err))?;
    if let Err(err) = fs::rename(&made, &link) {
        fs::remove_file(&made).ok();
        return Err(cant(program, &link, &err));
    }
    Ok(link)
}

/// Why `link` can't be made, and how to make it instead.
fn cant(program: &Path, link: &Path, err: &io::Error) -> Error {
    Error::failed(format!(
        "can't link {}: {err}. In a terminal: sudo ln -sf '{}' {}",
        link.display(),
        program.display(),
        link.display()
    ))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A folder of the test's own, empty, removed as the test ends, pass
    /// or fail, even shut.
    struct Folder(PathBuf);

    impl std::ops::Deref for Folder {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700)).ok();
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn folder(name: &str) -> Folder {
        let folder = std::env::temp_dir()
            .join("plays-tests")
            .join(format!("cli-{name}-{}", std::process::id()));
        fs::remove_dir_all(&folder).ok();
        fs::create_dir_all(&folder).unwrap();
        Folder(folder)
    }

    #[test]
    fn plays_is_linked_where_a_terminal_finds_it() {
        let folder = folder("link");
        let program = Path::new("/Applications/Plays.app/Contents/MacOS/plays");
        let link = link(program, &folder).unwrap();
        assert_eq!(link, folder.join("plays"));
        assert_eq!(fs::read_link(&link).unwrap(), program);

        let moved = Path::new("/Users/me/Applications/Plays.app/Contents/MacOS/plays");
        link_again(moved, &folder);
        let names: Vec<_> = fs::read_dir(&*folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["plays"], "nothing left beside it");
    }

    /// Link `program` where a link is already, which it replaces.
    fn link_again(program: &Path, folder: &Path) {
        let link = link(program, folder).unwrap();
        assert_eq!(fs::read_link(link).unwrap(), program, "the link replaced");
    }

    #[test]
    fn a_tests_folder_is_removed_as_it_ends() {
        let shut = folder("gone");
        let path = shut.to_path_buf();
        fs::write(path.join("plays"), "").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).unwrap();
        drop(shut);
        assert!(!path.exists(), "{} is left", path.display());
    }

    #[test]
    fn what_isnt_a_link_is_left_there() {
        let folder = folder("file");
        fs::write(folder.join("plays"), "another program").unwrap();
        let err = link(Path::new("/Applications/Plays.app/plays"), &folder).unwrap_err();
        assert!(err.to_string().contains("isn't a link"), "{err}");
        assert_eq!(
            fs::read_to_string(folder.join("plays")).unwrap(),
            "another program"
        );
    }

    #[test]
    fn a_folder_it_cant_write_to_says_what_to_type_instead() {
        let folder = folder("shut");
        fs::set_permissions(&*folder, fs::Permissions::from_mode(0o500)).unwrap();
        let program = Path::new("/Applications/Plays.app/Contents/MacOS/plays");
        let err = link(program, &folder).unwrap_err().to_string();
        let typed = format!(
            "sudo ln -sf '{}' {}",
            program.display(),
            folder.join("plays").display()
        );
        assert!(
            err.contains("ermission denied") && err.ends_with(&typed),
            "{err}"
        );
    }
}
