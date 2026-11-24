//! What macOS is told of the formats: the document types Finder offers
//! Plays for, each with its icon, and the type IDs Plays declares. A
//! property list `script/bundle-macos` merges into the app's Info.plist.

use std::fmt::Write;

use crate::{FORMATS, Format, Kind, MacType};

/// Where `script/file-types` writes `document_types`, from the workspace.
pub const DOCUMENT_TYPES: &str = "resources/macos/DocumentTypes.plist";

/// The icons the document types point at, each an `.icns` in the app's
/// Resources, by name, and the kind of format: a format's by its
/// extension, then video and audio, for files whose type ID another app
/// declared.
pub fn icons() -> Vec<(&'static str, Kind)> {
    FORMATS
        .iter()
        .filter(|format| format.mac.is_some())
        .map(|format| (format.extension(), format.kind))
        .chain([Kind::Video, Kind::Audio].map(|kind| (kind.name(), kind)))
        .collect()
}

/// The property list of the document types and declared type IDs.
pub fn document_types() -> String {
    let mut plist = String::from(HEADER);
    plist.push_str("\t<key>CFBundleDocumentTypes</key>\n\t<array>\n");
    for format in FORMATS {
        if let Some(mac) = format.mac {
            document(&mut plist, format.name, mac.id(), Some(format.extension()));
        }
    }
    document(
        &mut plist,
        "Video",
        "public.movie",
        Some(Kind::Video.name()),
    );
    document(
        &mut plist,
        "Audio",
        "public.audio",
        Some(Kind::Audio.name()),
    );
    document(&mut plist, "Folder", "public.folder", None);
    plist.push_str("\t</array>\n");
    plist.push_str("\t<key>UTImportedTypeDeclarations</key>\n\t<array>\n");
    for format in FORMATS {
        if let Some(MacType::Declared(id)) = format.mac {
            declaration(&mut plist, format, id);
        }
    }
    plist.push_str("\t</array>\n</dict>\n</plist>\n");
    plist
}

const HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- Written by script/file-types from crates/file-types; merged into
     Info.plist by script/bundle-macos.

     What Finder can open with Plays, and what can be dropped on its Dock
     icon, each with the icon a file gets when Plays is its default player.
     Alternate: Plays shows up under Open With without becoming the default.
     Video and Audio take the formats whose type ID is another app's.
     Folders can only be dropped (rank None).

     Then the type IDs macOS may not know. Imported, so a declaration from
     the system or another app wins where there is one. -->
<plist version="1.0">
<dict>
"#;

/// A document type of `id`, with the icon `icon` (none for folders, which
/// can only be dropped).
fn document(plist: &mut String, name: &str, id: &str, icon: Option<&str>) {
    let rank = if icon.is_some() { "Alternate" } else { "None" };
    plist.push_str("\t\t<dict>\n");
    entry(plist, 3, "CFBundleTypeName", name);
    if let Some(icon) = icon {
        entry(plist, 3, "CFBundleTypeIconFile", icon);
    }
    entry(plist, 3, "CFBundleTypeRole", "Viewer");
    entry(plist, 3, "LSHandlerRank", rank);
    list(plist, 3, "LSItemContentTypes", &[id]);
    plist.push_str("\t\t</dict>\n");
}

/// The declaration of `id`, the type ID of `format`.
fn declaration(plist: &mut String, format: &Format, id: &str) {
    let conforms = match format.kind {
        Kind::Video => "public.movie",
        Kind::Audio => "public.audio",
    };
    plist.push_str("\t\t<dict>\n");
    entry(plist, 3, "UTTypeIdentifier", id);
    entry(plist, 3, "UTTypeDescription", format.name);
    list(plist, 3, "UTTypeConformsTo", &[conforms]);
    plist.push_str("\t\t\t<key>UTTypeTagSpecification</key>\n\t\t\t<dict>\n");
    list(plist, 4, "public.filename-extension", format.extensions);
    list(plist, 4, "public.mime-type", format.mime);
    plist.push_str("\t\t\t</dict>\n\t\t</dict>\n");
}

// Nothing in the formats needs escaping in XML: the property list test
// reads it back.
fn entry(plist: &mut String, depth: usize, key: &str, value: &str) {
    let tabs = "\t".repeat(depth);
    writeln!(
        plist,
        "{tabs}<key>{key}</key>\n{tabs}<string>{value}</string>"
    )
    .unwrap();
}

fn list(plist: &mut String, depth: usize, key: &str, values: &[&str]) {
    let tabs = "\t".repeat(depth);
    writeln!(plist, "{tabs}<key>{key}</key>\n{tabs}<array>").unwrap();
    for value in values {
        writeln!(plist, "{tabs}\t<string>{value}</string>").unwrap();
    }
    writeln!(plist, "{tabs}</array>").unwrap();
}
