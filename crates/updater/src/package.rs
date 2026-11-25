//! Update files: an install, a folder (an app bundle) or a single file (a
//! binary), as each of its files kept from the installed version, patched
//! from it, or whole. One with no base is the whole install; a delta has
//! an earlier version as its base, and applies only to it, file by file.
//!
//! Laid out as `UPDATER1`, the header's length (8 bytes, little-endian),
//! the header as JSON, then the blobs it points into: zstd frames, each a
//! file whole, or patched with the installed file as zstd's reference.
//! Every file written is checked against its size and SHA-256, and every
//! base file against its SHA-256 before it's used.

use std::collections::HashSet;
use std::fs;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hash::{self, Hashing};
use crate::{Error, Result};

const MAGIC: &[u8; 8] = b"UPDATER1";
/// The most a header may be, in bytes.
const HEADER_LIMIT: u64 = 16 << 20;
/// The farthest back a zstd frame may refer: 1 GiB, covering the largest
/// file patched, which is its reference.
const WINDOW_LOG_MAX: u32 = 30;

#[derive(Debug, Serialize, Deserialize)]
struct Header {
    /// The version it updates from, for a delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    to: String,
    root: Root,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Root {
    /// A single file, such as a binary.
    File(FileEntry),
    /// A folder, such as an app bundle, and what's in it: each folder
    /// before what's in it.
    Dir { mode: u32, entries: Vec<Entry> },
}

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    /// Within the folder, `/` between names.
    path: String,
    #[serde(flatten)]
    kind: Kind,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Kind {
    Dir {
        mode: u32,
    },
    /// A link within the folder.
    Symlink {
        target: String,
    },
    File(FileEntry),
}

#[derive(Debug, Serialize, Deserialize)]
struct FileEntry {
    mode: u32,
    size: u64,
    sha256: String,
    from: Source,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Source {
    Whole(Blob),
    /// The installed file at the same path, as it is: its SHA-256.
    Same {
        base: String,
    },
    /// The installed file at the same path, patched.
    Patch {
        base: String,
        blob: Blob,
    },
}

/// Where a zstd frame is, from the end of the header.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Blob {
    offset: u64,
    len: u64,
}

/// What an update file made has: how many files are kept, patched and
/// whole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Made {
    pub kept: usize,
    pub patched: usize,
    pub whole: usize,
}

/// The versions an update file is from, if it's a delta, and to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Versions {
    pub from: Option<String>,
    pub to: String,
}

/// Make the update file `out` for the install at `new`, version `to`: a
/// delta from `base`, version `from`, if there is one, else whole. Files
/// are compressed at zstd's `level` (19 for a release).
pub fn make(
    new: &Path,
    base: Option<(&Path, &str)>,
    to: &str,
    level: i32,
    out: &Path,
) -> Result<Made> {
    let mut making = Making {
        base: base.map(|(path, _)| path),
        level,
        blobs: Vec::new(),
        made: Made::default(),
    };
    let root = if fs::symlink_metadata(new)?.is_dir() {
        let mut entries = Vec::new();
        making.walk(new, "", &mut entries)?;
        Root::Dir {
            mode: mode(&fs::metadata(new)?),
            entries,
        }
    } else {
        let base = making.base.map(Path::to_owned);
        Root::File(making.file(new, base)?)
    };
    let header = Header {
        from: base.map(|(_, from)| from.to_owned()),
        to: to.to_owned(),
        root,
    };
    let header = serde_json::to_vec(&header).map_err(io::Error::other)?;
    let partial = with_suffix(out, ".partial");
    let mut file = BufWriter::new(fs::File::create(&partial)?);
    file.write_all(MAGIC)?;
    file.write_all(&(header.len() as u64).to_le_bytes())?;
    file.write_all(&header)?;
    file.write_all(&making.blobs)?;
    file.into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()?;
    fs::rename(&partial, out)?;
    Ok(making.made)
}

struct Making<'a> {
    base: Option<&'a Path>,
    level: i32,
    blobs: Vec<u8>,
    made: Made,
}

impl Making<'_> {
    /// The entries for what's in `folder`, at `prefix` in the install, by
    /// name.
    fn walk(&mut self, folder: &Path, prefix: &str, entries: &mut Vec<Entry>) -> Result<()> {
        let mut names: Vec<_> = fs::read_dir(folder)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<_>>()?;
        names.sort();
        for name in names {
            let name = name.to_str().ok_or_else(|| {
                Error::update(format!("{} isn't UTF-8", folder.join(&name).display()))
            })?;
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let on_disk = folder.join(name);
            let metadata = fs::symlink_metadata(&on_disk)?;
            let kind = if metadata.is_dir() {
                Kind::Dir {
                    mode: mode(&metadata),
                }
            } else if metadata.is_symlink() {
                let target = fs::read_link(&on_disk)?;
                let target = target
                    .to_str()
                    .filter(|target| link_stays_inside(&path, target))
                    .ok_or_else(|| Error::update(format!("{path} links out of the install")))?;
                Kind::Symlink {
                    target: target.to_owned(),
                }
            } else {
                let base = self.base.map(|base| base.join(&path));
                Kind::File(self.file(&on_disk, base)?)
            };
            let is_dir = matches!(kind, Kind::Dir { .. });
            entries.push(Entry {
                path: path.clone(),
                kind,
            });
            if is_dir {
                self.walk(&on_disk, &path, entries)?;
            }
        }
        Ok(())
    }

    /// The file at `path`: kept or patched from `base`, if that's a file,
    /// whichever's smaller, else whole.
    fn file(&mut self, path: &Path, base: Option<PathBuf>) -> Result<FileEntry> {
        let bytes = fs::read(path)?;
        let sha256 = hash::of(&bytes);
        let base = base
            .filter(|base| base.is_file())
            .map(fs::read)
            .transpose()?;
        let from = match base {
            Some(base) if hash::of(&base) == sha256 => {
                self.made.kept += 1;
                Source::Same {
                    base: sha256.clone(),
                }
            }
            Some(base) => {
                let patch = compress(&bytes, &base, self.level)?;
                let whole = compress(&bytes, &[], self.level)?;
                if patch.len() < whole.len() {
                    self.made.patched += 1;
                    Source::Patch {
                        base: hash::of(&base),
                        blob: self.blob(&patch),
                    }
                } else {
                    self.made.whole += 1;
                    Source::Whole(self.blob(&whole))
                }
            }
            None => {
                self.made.whole += 1;
                let whole = compress(&bytes, &[], self.level)?;
                Source::Whole(self.blob(&whole))
            }
        };
        Ok(FileEntry {
            mode: mode(&fs::metadata(path)?),
            size: bytes.len() as u64,
            sha256,
            from,
        })
    }

    fn blob(&mut self, frame: &[u8]) -> Blob {
        let blob = Blob {
            offset: self.blobs.len() as u64,
            len: frame.len() as u64,
        };
        self.blobs.extend_from_slice(frame);
        blob
    }
}

/// `bytes` as a zstd frame, with `reference` as its prefix: zstd's patch
/// from it.
fn compress(bytes: &[u8], reference: &[u8], level: i32) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::stream::write::Encoder::with_ref_prefix(Vec::new(), level, reference)?;
    let reach = bytes.len().max(reference.len()).max(1);
    // Far enough back to reach all of the reference.
    let window_log = (usize::BITS - reach.leading_zeros() + 1).clamp(10, WINDOW_LOG_MAX);
    encoder.window_log(window_log)?;
    encoder.long_distance_matching(reach > 1 << 20)?;
    encoder.set_pledged_src_size(Some(bytes.len() as u64))?;
    encoder.include_checksum(true)?;
    encoder.write_all(bytes)?;
    encoder.finish()
}

/// The versions the update file at `path` is from and to.
pub fn versions(path: &Path) -> Result<Versions> {
    let mut file = fs::File::open(path)?;
    let header = read_header(&mut file)?;
    Ok(Versions {
        from: header.from,
        to: header.to,
    })
}

/// Apply the update file at `update`, version `to`, into `out`, which
/// mustn't exist, kept and patched files read from the install at `base`.
/// Nothing is left at `out` if it fails.
pub fn apply(update: &Path, base: Option<&Path>, to: &str, out: &Path) -> Result<()> {
    if fs::symlink_metadata(out).is_ok() {
        return Err(Error::update(format!("{} is there already", out.display())));
    }
    let applied = apply_into(update, base, to, out);
    if applied.is_err() {
        remove(out);
    }
    applied
}

fn apply_into(update: &Path, base: Option<&Path>, to: &str, out: &Path) -> Result<()> {
    let mut file = fs::File::open(update)?;
    let header = read_header(&mut file)?;
    if header.to != to {
        return Err(Error::update(format!("it's {}'s, not {to}'s", header.to)));
    }
    let mut applying = Applying {
        file,
        blobs: 0,
        length: 0,
    };
    applying.blobs = applying.file.stream_position()?;
    applying.length = applying.file.metadata()?.len();
    match &header.root {
        Root::File(entry) => applying.file_entry("", entry, base.map(Path::to_owned), out),
        Root::Dir { mode, entries } => applying.dir(*mode, entries, base, out),
    }
}

fn read_header(file: &mut fs::File) -> Result<Header> {
    let mut magic = [0; 8];
    let mut length = [0; 8];
    if file.read_exact(&mut magic).is_err()
        || &magic != MAGIC
        || file.read_exact(&mut length).is_err()
    {
        return Err(Error::update("it isn't an update file"));
    }
    let length = u64::from_le_bytes(length);
    if length > HEADER_LIMIT {
        return Err(Error::update("its header is too big"));
    }
    let mut header = Vec::new();
    file.take(length).read_to_end(&mut header)?;
    serde_json::from_slice(&header).map_err(|err| Error::update(format!("its header: {err}")))
}

struct Applying {
    file: fs::File,
    /// Where the blobs start.
    blobs: u64,
    /// The update file's.
    length: u64,
}

impl Applying {
    fn dir(&mut self, mode: u32, entries: &[Entry], base: Option<&Path>, out: &Path) -> Result<()> {
        fs::create_dir(out)?;
        let mut dirs = HashSet::from([String::new()]);
        let mut links = HashSet::new();
        let mut seen = HashSet::new();
        let mut modes = vec![(out.to_owned(), mode)];
        for entry in entries {
            let path = entry.path.as_str();
            let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
            if !valid_path(path)
                || !seen.insert(path)
                || !dirs.contains(parent)
                || links.contains(parent)
            {
                return Err(Error::update(format!("{path:?} can't be in it")));
            }
            let on_disk = out.join(path);
            match &entry.kind {
                Kind::Dir { mode } => {
                    fs::create_dir(&on_disk)?;
                    dirs.insert(path.to_owned());
                    modes.push((on_disk, *mode));
                }
                Kind::Symlink { target } => {
                    if !link_stays_inside(path, target) {
                        return Err(Error::update(format!("{path} links out of the install")));
                    }
                    symlink(target, &on_disk)?;
                    links.insert(path.to_owned());
                }
                Kind::File(file) => {
                    self.file_entry(path, file, base.map(|base| base.join(path)), &on_disk)?;
                }
            }
        }
        // Last, as a folder that can't be written to can't be filled.
        for (dir, mode) in modes.iter().rev() {
            set_mode(dir, *mode)?;
        }
        Ok(())
    }

    /// The file `entry` at `path` in the install, into `out`, from the
    /// installed one at `base` if it's kept or patched.
    fn file_entry(
        &mut self,
        path: &str,
        entry: &FileEntry,
        base: Option<PathBuf>,
        out: &Path,
    ) -> Result<()> {
        let mut written = Hashing::new(BufWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(out)?,
        ));
        let not_from = |why: String| Error::update(format!("the installed {} {why}", shown(path)));
        match &entry.from {
            Source::Whole(blob) => {
                let mut decoder = zstd::stream::read::Decoder::new(self.blob(*blob)?)?;
                decoder.window_log_max(WINDOW_LOG_MAX)?;
                copy_file(&mut decoder, &mut written, entry.size, path)?;
            }
            Source::Same { .. } => {
                let base =
                    base.ok_or_else(|| Error::update("it's a delta, and there's no install"))?;
                let mut base = fs::File::open(&base)
                    .map_err(|err| not_from(format!("can't be read: {err}")))?;
                copy_file(&mut base, &mut written, entry.size, path)
                    .map_err(|_| not_from("isn't the one it's from".into()))?;
            }
            Source::Patch {
                base: expected,
                blob,
            } => {
                let base =
                    base.ok_or_else(|| Error::update("it's a delta, and there's no install"))?;
                let reference =
                    fs::read(&base).map_err(|err| not_from(format!("can't be read: {err}")))?;
                if !hash::same(&hash::of(&reference), expected) {
                    return Err(not_from("isn't the one it's from".into()));
                }
                let mut decoder = zstd::stream::read::Decoder::with_ref_prefix(
                    io::BufReader::new(self.blob(*blob)?),
                    &reference,
                )?;
                decoder.window_log_max(WINDOW_LOG_MAX)?;
                copy_file(&mut decoder, &mut written, entry.size, path)?;
            }
        }
        let size = written.written();
        let (sha, writer) = written.finish();
        let matches = size == entry.size && hash::same(&sha, &entry.sha256);
        match &entry.from {
            // What was copied is the installed file.
            Source::Same { base: expected } if !matches || !hash::same(&sha, expected) => {
                return Err(not_from("isn't the one it's from".into()));
            }
            _ if !matches => return Err(Error::Mismatch(shown(path).to_owned())),
            _ => {}
        }
        writer
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()?;
        set_mode(out, entry.mode)?;
        Ok(())
    }

    /// The blob's bytes.
    fn blob(&mut self, blob: Blob) -> Result<io::Take<&mut fs::File>> {
        let start = self.blobs.checked_add(blob.offset);
        let end = start.and_then(|start| start.checked_add(blob.len));
        match (start, end) {
            (Some(start), Some(end)) if end <= self.length => {
                self.file.seek(SeekFrom::Start(start))?;
                Ok((&mut self.file).take(blob.len))
            }
            _ => Err(Error::update("it's cut short")),
        }
    }
}

/// `path` as said: the install itself when it's a single file.
fn shown(path: &str) -> &str {
    if path.is_empty() { "file" } else { path }
}

/// Copy `size` bytes of `path`'s from `from`, no more.
fn copy_file(from: &mut impl Read, to: &mut impl Write, size: u64, path: &str) -> Result<()> {
    hash::copy_at_most(from, to, size).map_err(|err| match err.kind() {
        io::ErrorKind::FileTooLarge => Error::Mismatch(shown(path).to_owned()),
        _ => Error::update(format!("{}: {err}", shown(path))),
    })?;
    Ok(())
}

/// Whether `path` is one an install can have: names between `/`, none
/// empty, `.` or `..`, and none with `\`, `:` or NUL, which could name
/// something else on Windows.
pub(crate) fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|name| {
            !name.is_empty() && name != "." && name != ".." && !name.contains(['\\', ':', '\0'])
        })
}

/// Whether a link at `path` to `target` stays within the install.
pub(crate) fn link_stays_inside(path: &str, target: &str) -> bool {
    if target.is_empty() || target.starts_with('/') || target.contains(['\\', ':', '\0']) {
        return false;
    }
    let mut depth = path.matches('/').count();
    for name in target.split('/') {
        match name {
            "" | "." => {}
            ".." => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return false,
            },
            _ => depth += 1,
        }
    }
    true
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    path.with_file_name(name)
}

/// Whatever is at `path`, gone.
pub(crate) fn remove(path: &Path) {
    let removed = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            // A folder made read-only can't be emptied.
            make_writable(path);
            fs::remove_dir_all(path)
        }
        Ok(_) => fs::remove_file(path),
        Err(_) => Ok(()),
    };
    if let Err(err) = removed {
        log::warn!("can't remove {}: {err}", path.display());
    }
}

#[cfg(unix)]
fn mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn mode(metadata: &fs::Metadata) -> u32 {
    if metadata.is_dir() { 0o755 } else { 0o644 }
}

/// `path`'s permissions as `mode` says, set-user and set-group bits never.
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_writable(folder: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut folders = vec![folder.to_owned()];
    while let Some(folder) = folders.pop() {
        let _ = fs::set_permissions(&folder, fs::Permissions::from_mode(0o755));
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                folders.push(entry.path());
            }
        }
    }
}

#[cfg(not(unix))]
fn make_writable(_: &Path) {}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_: &str, link: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{} is a link, which an install here can't have",
            link.display()
        ),
    ))
}

#[cfg(all(test, unix))]
#[path = "tests/package.rs"]
pub(crate) mod tests;

#[cfg(all(test, unix))]
#[path = "tests/package_properties.rs"]
mod properties;
