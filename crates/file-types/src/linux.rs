//! What Linux is told of the formats: the MIME types in Plays' desktop
//! file, which Open With offers it for, and an icon for each, named as an
//! icon theme names a MIME type's.

use crate::{FORMATS, Format};

/// Plays' desktop file, from the workspace.
pub const DESKTOP_FILE: &str = "resources/linux/plays.desktop";

/// Where `script/file-types` writes the icons, from the workspace: laid
/// out as in `~/.local/share/icons`.
pub const ICONS: &str = "resources/linux/icons/hicolor/scalable/mimetypes";

/// The desktop file's `MimeType=` value: every format's MIME types.
pub fn mime_types() -> String {
    FORMATS
        .iter()
        .flat_map(|format| format.mime)
        .map(|mime| format!("{mime};"))
        .collect()
}

/// The desktop file `desktop_file` with its `MimeType=` line, or a new
/// one at the end, giving `mime_types`.
pub fn desktop_file(desktop_file: &str) -> String {
    let line = format!("MimeType={}", mime_types());
    let mut lines: Vec<&str> = desktop_file.lines().collect();
    match lines.iter().position(|l| l.starts_with("MimeType=")) {
        Some(i) => lines[i] = &line,
        None => lines.push(&line),
    }
    lines.iter().map(|l| format!("{l}\n")).collect()
}

/// Each MIME type's icon, by file name (`video/x-matroska` is
/// `video-x-matroska.svg`), and its format.
pub fn icons() -> Vec<(String, &'static Format)> {
    FORMATS
        .iter()
        .flat_map(|format| {
            format
                .mime
                .iter()
                .map(move |mime| (format!("{}.svg", mime.replace('/', "-")), format))
        })
        .collect()
}
