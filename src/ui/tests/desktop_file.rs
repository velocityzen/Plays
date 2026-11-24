//! Linux ties the windows to Plays' desktop file by their app_id, and
//! finds the icon by the name the desktop file gives it.

use std::fs;
use std::path::Path;

use super::APP_ID;

#[test]
fn the_app_id_names_the_desktop_file_whose_icon_is_drawn() {
    let linux = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/linux");
    let desktop_file = fs::read_to_string(linux.join(format!("{APP_ID}.desktop"))).unwrap();
    let value = |key: &str| {
        let mut values = desktop_file
            .lines()
            .filter_map(|line| line.strip_prefix(key)?.strip_prefix('='));
        values.next().unwrap_or_else(|| panic!("no {key}="))
    };
    assert_eq!(value("StartupWMClass"), APP_ID);
    let icon = linux.join(format!("icons/hicolor/scalable/apps/{}.svg", value("Icon")));
    let svg = fs::read_to_string(&icon).unwrap();
    assert!(svg.starts_with("<svg"), "{}", icon.display());
}
