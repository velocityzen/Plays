use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::*;

/// The workspace's `path`.
fn workspace(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// The SVGs in the workspace's `folder`, by name.
fn svgs_in(folder: &str) -> BTreeSet<String> {
    let entries = fs::read_dir(workspace(folder)).unwrap();
    let names = entries.map(|entry| entry.unwrap().file_name().into_string().unwrap());
    names.filter(|name| name.ends_with(".svg")).collect()
}

const RERUN: &str = "out of date with crates/file-types: run script/file-types";

#[test]
fn each_extension_is_one_formats_in_lowercase() {
    let mut seen = HashSet::new();
    for extension in FORMATS.iter().flat_map(|format| format.extensions) {
        assert_eq!(*extension, extension.to_ascii_lowercase());
        assert!(seen.insert(extension), "{extension} is in two formats");
    }
}

#[test]
fn each_mime_type_and_type_id_is_one_formats() {
    let mut mime = HashSet::new();
    for each in FORMATS.iter().flat_map(|format| format.mime) {
        assert!(mime.insert(each), "{each} is in two formats");
    }
    let mut ids = HashSet::new();
    for id in FORMATS
        .iter()
        .filter_map(|format| format.mac)
        .map(MacType::id)
    {
        assert!(ids.insert(id), "{id} is in two formats");
    }
}

#[test]
fn an_extension_fits_on_its_icon() {
    for format in FORMATS {
        assert!(format.extension().len() <= 4, "{}", format.extension());
    }
}

#[test]
fn a_format_is_found_by_any_of_its_extensions_in_any_case() {
    assert_eq!(format_of("MKV").map(|f| f.name), Some("Matroska video"));
    assert_eq!(format_of("mpeg").map(Format::extension), Some("mpg"));
    assert_eq!(format_of("Oga").map(|f| f.kind), Some(Kind::Audio));
    assert_eq!(format_of("srt"), None);
    assert_eq!(format_of(""), None);
}

#[test]
fn what_isnt_claimed_is_only_listed() {
    let listed: Vec<_> = FORMATS.iter().filter(|f| !f.is_claimed()).collect();
    let extensions: Vec<_> = listed.iter().map(|f| f.extension()).collect();
    assert_eq!(extensions, ["gif", "vob", "alac"]);
    let names = icon_names();
    assert!(extensions.iter().all(|e| !names.contains(e)));
    assert!(names.ends_with(&["video", "audio"]));
}

#[test]
fn the_document_types_written_are_the_formats() {
    let written = fs::read_to_string(workspace(macos::DOCUMENT_TYPES)).unwrap();
    assert!(
        written == macos::document_types(),
        "{} {RERUN}",
        macos::DOCUMENT_TYPES
    );
}

#[test]
fn the_document_types_are_a_property_list_of_each_format() {
    let plist = plist::Value::from_reader_xml(macos::document_types().as_bytes()).unwrap();
    let plist = plist.as_dictionary().unwrap();
    let types = plist["CFBundleDocumentTypes"].as_array().unwrap();
    let string = |dict: &plist::Dictionary, key: &str| dict[key].as_string().unwrap().to_owned();
    let mut icons = Vec::new();
    for each in types {
        let each = each.as_dictionary().unwrap();
        assert_eq!(string(each, "CFBundleTypeRole"), "Viewer");
        if let Some(icon) = each.get("CFBundleTypeIconFile") {
            icons.push(icon.as_string().unwrap().to_owned());
            assert_eq!(string(each, "LSHandlerRank"), "Alternate");
        } else {
            assert_eq!(string(each, "LSHandlerRank"), "None");
        }
    }
    let names: Vec<_> = macos::icons().into_iter().map(|(name, _)| name).collect();
    assert_eq!(icons, names);
    let mkv = &types[icons.iter().position(|i| i == "mkv").unwrap()];
    let mkv = mkv.as_dictionary().unwrap()["LSItemContentTypes"].as_array();
    assert_eq!(mkv.unwrap()[0].as_string(), Some("org.matroska.mkv"));

    let declared = plist["UTImportedTypeDeclarations"].as_array().unwrap();
    let ogg = declared.iter().map(|d| d.as_dictionary().unwrap());
    let ogg = ogg
        .into_iter()
        .find(|d| string(d, "UTTypeIdentifier") == "org.xiph.ogg-audio")
        .unwrap();
    assert_eq!(string(ogg, "UTTypeDescription"), "Ogg audio");
    let tags = ogg["UTTypeTagSpecification"].as_dictionary().unwrap();
    let extensions = tags["public.filename-extension"].as_array().unwrap();
    let extensions: Vec<_> = extensions.iter().map(|e| e.as_string().unwrap()).collect();
    assert_eq!(extensions, ["ogg", "oga"]);
    let ape = declared.iter().map(|d| d.as_dictionary().unwrap());
    assert!(
        ape.into_iter()
            .any(|d| string(d, "UTTypeDescription") == "Monkey's Audio")
    );
}

#[test]
fn the_desktop_file_gives_every_formats_mime_types() {
    let written = fs::read_to_string(workspace(linux::DESKTOP_FILE)).unwrap();
    assert!(
        written == linux::desktop_file(&written),
        "{} {RERUN}",
        linux::DESKTOP_FILE
    );
    let lines = written.lines().filter(|l| l.starts_with("MimeType="));
    assert_eq!(lines.count(), 1);
    let mime = linux::mime_types();
    assert!(
        mime.starts_with("video/3gpp;video/vnd.avi;video/x-msvideo;"),
        "{mime}"
    );
    assert!(mime.ends_with(";audio/x-ms-wma;audio/x-wavpack;"), "{mime}");
}

#[test]
fn a_desktop_file_without_mime_types_gets_them_at_the_end() {
    let desktop_file = linux::desktop_file("[Desktop Entry]\nName=Plays");
    let expected = format!(
        "[Desktop Entry]\nName=Plays\nMimeType={}\n",
        linux::mime_types()
    );
    assert_eq!(desktop_file, expected);
}

#[test]
fn the_icons_written_are_the_formats() {
    let expected: BTreeSet<_> = icon_names().iter().map(|n| format!("{n}.svg")).collect();
    assert!(svgs_in(ICONS) == expected, "{ICONS} {RERUN}");
    let expected: BTreeSet<_> = linux::icons().into_iter().map(|(file, _)| file).collect();
    assert!(
        svgs_in(linux::ICONS) == expected,
        "{} {RERUN}",
        linux::ICONS
    );
    assert!(expected.contains("audio-x-vorbis+ogg.svg"));
}

#[test]
fn a_mime_types_icon_is_its_formats() {
    for (file, format) in linux::icons() {
        let linux = fs::read(workspace(linux::ICONS).join(&file)).unwrap();
        let svg = format!("{}.svg", format.extension());
        assert!(
            linux == fs::read(workspace(ICONS).join(svg)).unwrap(),
            "{file} {RERUN}"
        );
    }
}
