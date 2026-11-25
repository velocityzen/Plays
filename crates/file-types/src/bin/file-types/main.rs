//! `file-types write [--font FILE]` writes, from the formats Plays opens,
//! what macOS and Linux are told of them, and an icon for each format,
//! its extension in the Fira font found (or `FILE`). `file-types iconsets
//! FOLDER` draws the icons macOS's document types point at, as iconsets
//! in `FOLDER` for `iconutil` to make `.icns` of. `script/file-types` and
//! `script/bundle-macos` run them.

#![forbid(unsafe_code)]

mod icon;
mod iconset;

use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use file_types::{FORMATS, Format, Kind, linux, macos};

use crate::icon::Labels;

type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "usage: file-types write [--font FILE]\n       file-types iconsets FOLDER";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let done = match args.as_slice() {
        ["write"] => write(None),
        ["write", "--font", font] => write(Some(Path::new(font))),
        ["iconsets", folder] => iconsets(Path::new(folder)),
        _ => Err(USAGE.to_owned()),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("file-types: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Writes the icons, the document types and the desktop file's MIME types.
fn write(font: Option<&Path>) -> Result<()> {
    let root = workspace();
    let labels = Labels::find(font)?;
    println!("Extensions in {}", labels.family());
    let mut icons = Vec::new();
    for format in FORMATS.iter().filter(|format| format.is_claimed()) {
        let label = format.extension().to_ascii_uppercase();
        icons.push((format.extension(), labels.icon(format.kind, Some(&label))?));
    }
    for kind in [Kind::Video, Kind::Audio] {
        icons.push((kind.name(), labels.icon(kind, None)?));
    }
    let named = |name: &str| {
        let (_, svg) = icons.iter().find(|(n, _)| *n == name).expect("an icon");
        svg.as_str()
    };
    let by_format = icons
        .iter()
        .map(|(name, svg)| (format!("{name}.svg"), svg.as_str()));
    replace_svgs(&root.join(file_types::ICONS), by_format)?;
    let by_mime = linux::icons().into_iter();
    let by_mime =
        by_mime.map(|(file, format): (String, &Format)| (file, named(format.extension())));
    replace_svgs(&root.join(linux::ICONS), by_mime)?;
    let document_types = root.join(macos::DOCUMENT_TYPES);
    fs::write(&document_types, macos::document_types()).map_err(at(&document_types))?;
    let desktop = root.join(linux::DESKTOP_FILE);
    let desktop_file = fs::read_to_string(&desktop).map_err(at(&desktop))?;
    fs::write(&desktop, linux::desktop_file(&desktop_file)).map_err(at(&desktop))?;
    println!(
        "Wrote {} icons, {} for Linux's MIME types, {} and {}",
        icons.len(),
        linux::icons().len(),
        macos::DOCUMENT_TYPES,
        linux::DESKTOP_FILE
    );
    Ok(())
}

/// Writes `svgs`, by file name, into `folder`, in place of the SVGs there.
fn replace_svgs<'a>(folder: &Path, svgs: impl Iterator<Item = (String, &'a str)>) -> Result<()> {
    fs::create_dir_all(folder).map_err(at(folder))?;
    for entry in fs::read_dir(folder).map_err(at(folder))? {
        let path = entry.map_err(at(folder))?.path();
        if path.extension().is_some_and(|e| e == "svg") {
            fs::remove_file(&path).map_err(at(&path))?;
        }
    }
    for (name, svg) in svgs {
        let path = folder.join(name);
        fs::write(&path, svg).map_err(at(&path))?;
    }
    Ok(())
}

/// Writes the icons macOS's document types point at into `folder`, each
/// as `<name>.iconset`, drawn as its kind's below the size a label can be
/// read at.
fn iconsets(folder: &Path) -> Result<()> {
    let icons = workspace().join(file_types::ICONS);
    let read = |name: &str| {
        let svg = icons.join(format!("{name}.svg"));
        fs::read_to_string(&svg).map_err(at(&svg))
    };
    for (name, kind) in macos::icons() {
        let iconset = folder.join(format!("{name}.iconset"));
        iconset::write(&iconset, &read(name)?, &read(kind.name())?)?;
    }
    Ok(())
}

/// The workspace this crate is in, which the files' paths are from.
fn workspace() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .expect("crates/file-types")
        .into()
}

/// An error about `path`, saying so.
fn at<E: Display>(path: &Path) -> impl FnOnce(E) -> String {
    move |error| format!("{}: {error}", path.display())
}
