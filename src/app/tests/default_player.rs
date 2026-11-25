use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use file_types::format_of;

use super::xdg::Xdg;
use super::*;
use crate::files::test_folder;

fn format(extension: &str) -> &'static Format {
    format_of(extension).unwrap()
}

#[test]
fn what_came_of_it_reads_as_sentences() {
    let made = Made {
        set: vec![format("mkv"), format("webm"), format("mp4")],
        not_set: vec![(format("m4v"), "VLC still opens them".to_owned())],
    };
    assert_eq!(
        made.says(),
        "Plays opens 3 of the 4 formats picked now. \
         Not MPEG-4 video (.m4v): VLC still opens them."
    );
    let all = Made {
        set: vec![format("mkv"), format("flac")],
        not_set: Vec::new(),
    };
    assert_eq!(all.says(), "Plays opens the 2 formats picked now.");
    let one = Made {
        set: vec![format("mkv")],
        not_set: Vec::new(),
    };
    assert_eq!(one.says(), "Plays opens the format picked now.");
    let none = Made {
        set: Vec::new(),
        not_set: vec![
            (format("mp3"), "VLC still opens them".to_owned()),
            (format("flac"), "macOS didn't change it".to_owned()),
        ],
    };
    assert_eq!(
        none.says(),
        "Not MP3 audio (.mp3): VLC still opens them. \
         Not FLAC audio (.flac): macOS didn't change it."
    );
    assert_eq!(Made::default().says(), "");
}

#[test]
fn a_format_says_who_opens_its_files() {
    let opener = |app: Option<&str>, plays| Opener {
        format: format("mkv"),
        app: app.map(str::to_owned),
        plays,
    };
    assert_eq!(opener(Some("VLC"), false).says(), "VLC opens them");
    assert_eq!(opener(Some("Plays"), true).says(), "Plays opens them");
    assert_eq!(opener(None, false).says(), "No app opens them");
}

/// A stand-in for `xdg-mime` in `folder`, keeping each MIME type's
/// desktop file in `folder/defaults`, a line each, the last one winning;
/// setting fails, as on a read-only config, if `folder/read-only` is
/// there. Also its `applications`, with VLC's desktop file in it.
fn stand_in(folder: &Path) -> Xdg {
    let program = folder.join("xdg-mime");
    fs::write(
        &program,
        r#"#!/bin/sh
here=$(dirname "$0")
case $1 in
query) grep "^$3 " "$here/defaults" 2>/dev/null | tail -n 1 | cut -d ' ' -f 2 ;;
default)
    if [ -e "$here/read-only" ]; then echo "mimeapps.list: Read-only file system" >&2; exit 4; fi
    id=$2; shift 2
    for mime in "$@"; do echo "$mime $id" >>"$here/defaults"; done ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let applications = folder.join("applications");
    fs::create_dir_all(&applications).unwrap();
    fs::write(
        applications.join("vlc.desktop"),
        "[Desktop Action play]\nName=Play\n\n[Desktop Entry]\nName=VLC media player\n",
    )
    .unwrap();
    Xdg {
        program,
        applications: vec![applications],
    }
}

/// Set `mime`'s desktop file in the stand-in's defaults.
fn set_default(folder: &Path, mime: &str, id: &str) {
    let defaults: PathBuf = folder.join("defaults");
    let mut lines = fs::read_to_string(&defaults).unwrap_or_default();
    lines.push_str(&format!("{mime} {id}\n"));
    fs::write(defaults, lines).unwrap();
}

#[test]
fn linux_names_the_app_each_format_opens_in() {
    let folder = test_folder("default-player-read");
    fs::create_dir_all(&folder).unwrap();
    let xdg = stand_in(&folder);
    set_default(&folder, "video/x-matroska", "vlc.desktop");
    set_default(&folder, "video/webm", "plays.desktop");
    // One of FLAC's MIME types is Plays', the other no one's.
    set_default(&folder, "audio/flac", "plays.desktop");
    set_default(&folder, "audio/mpeg", "rhythmbox.desktop");

    let openers = xdg.read().unwrap();
    let of = |extension| {
        openers
            .iter()
            .find(|opener| opener.format.extension() == extension)
            .unwrap()
    };
    assert_eq!(
        of("mkv").app.as_deref(),
        Some("VLC media player"),
        "its Name"
    );
    assert!(!of("mkv").plays);
    assert!(of("webm").plays);
    assert_eq!(of("flac").app, None, "audio/x-flac opens in no app");
    assert!(!of("flac").plays);
    assert_eq!(
        of("mp3").app.as_deref(),
        Some("rhythmbox"),
        "without its desktop file, by its id"
    );
    assert!(
        openers.iter().all(|opener| !opener.format.mime.is_empty()),
        "only formats with a MIME type: not .m2ts"
    );
}

#[test]
fn linux_sets_every_mime_type_of_the_formats_picked() {
    let folder = test_folder("default-player-make");
    fs::create_dir_all(&folder).unwrap();
    let xdg = stand_in(&folder);
    set_default(&folder, "video/x-matroska", "vlc.desktop");

    let made = xdg.make(&[format("mkv"), format("flac")]);
    assert_eq!(made.set, [format("mkv"), format("flac")]);
    assert!(made.not_set.is_empty());
    let defaults = fs::read_to_string(folder.join("defaults")).unwrap();
    for mime in ["video/x-matroska", "audio/flac", "audio/x-flac"] {
        assert!(
            defaults.contains(&format!("{mime} plays.desktop")),
            "{mime}"
        );
    }
    assert!(!defaults.contains("video/webm"), "only those picked");
}

#[test]
fn linux_says_why_a_format_wasnt_set() {
    let folder = test_folder("default-player-refused");
    fs::create_dir_all(&folder).unwrap();
    let xdg = stand_in(&folder);
    set_default(&folder, "video/x-matroska", "vlc.desktop");
    fs::write(folder.join("read-only"), "").unwrap();

    let made = xdg.make(&[format("mkv")]);
    assert!(made.set.is_empty());
    assert_eq!(
        made.not_set,
        [(
            format("mkv"),
            "xdg-mime: mimeapps.list: Read-only file system".to_owned()
        )]
    );
}

#[test]
fn linux_needs_plays_desktop_file_installed() {
    let folder = test_folder("default-player-desktop-file");
    fs::create_dir_all(&folder).unwrap();
    let xdg = stand_in(&folder);
    let cant = xdg.cant().expect("no plays.desktop");
    assert!(cant.contains("script/install-linux"), "{cant}");
    fs::write(
        folder.join("applications/plays.desktop"),
        "[Desktop Entry]\n",
    )
    .unwrap();
    assert_eq!(xdg.cant(), None);
}

/// What this Mac answers, which asks it nothing and changes nothing.
#[cfg(target_os = "macos")]
mod mac {
    use super::super::mac::{self, BUNDLE_ID};
    use super::format;

    #[test]
    fn an_extension_has_its_type_on_this_mac() {
        assert_eq!(mac::type_of("mp4").as_deref(), Some("public.mpeg-4"));
        assert_eq!(mac::type_of("mp3").as_deref(), Some("public.mp3"));
        assert_eq!(
            mac::type_of("plays-no-such-extension"),
            None,
            "not a made-up dyn. one"
        );
    }

    #[test]
    fn a_format_has_an_app_or_none_and_tests_arent_plays_app() {
        let opener = mac::opener(format("mp4"));
        assert_eq!(opener.format, format("mp4"));
        assert_eq!(opener.plays, opener.app.as_deref() == Some("Plays"));
        let cant = mac::cant().expect("a test isn't Plays.app");
        assert!(cant.contains("script/bundle-macos"), "{cant}");
    }

    #[test]
    fn the_bundle_identifier_is_info_plists() {
        let plist = include_str!("../../../resources/macos/Info.plist");
        let id = format!("<key>CFBundleIdentifier</key>\n\t<string>{BUNDLE_ID}</string>");
        assert!(plist.contains(&id), "Info.plist names another");
    }
}
