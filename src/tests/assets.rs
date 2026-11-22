use super::*;

/// An icon left out of `icons!` draws as nothing.
#[test]
fn every_icon_is_built_in() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().into_string().unwrap();
            name.strip_suffix(".svg").map(str::to_owned)
        })
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for name in names {
        assert!(
            Assets.load(&format!("icons/{name}.svg")).unwrap().is_some(),
            "assets/icons/{name}.svg isn't in `icons!`"
        );
    }
}

#[test]
fn the_logo_is_built_in() {
    let logo = Assets.load(LOGO).unwrap().expect("the logo");
    assert!(logo.starts_with(b"<svg"), "assets/icon.svg");
}
