//! Writing the app's own files: settings and where files were left.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Write `contents` to `path`, making its folder if need be. It goes to a
/// file beside it first, on the disk before it takes the name, so a crash
/// or losing power midway leaves the file as it was rather than half
/// written.
pub fn write_whole(path: &Path, contents: &[u8]) -> io::Result<()> {
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty());
    if let Some(folder) = folder {
        fs::create_dir_all(folder)?;
    }
    let beside = with_suffix(path, ".tmp");
    let written = File::create(&beside).and_then(|mut file| {
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

/// `path` with `suffix` after its whole name: `settings.json.bad`.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

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
}
