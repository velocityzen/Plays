use std::path::PathBuf;

use super::*;

/// A square of `color` filling a 256-wide icon.
fn square(color: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><rect width="256" height="256" fill="{color}"/></svg>"#
    )
}

/// The first pixel of the PNG at `path`, and its width and height.
fn first_pixel(path: &PathBuf) -> ([u8; 4], (u32, u32)) {
    let pixmap = Pixmap::load_png(path).unwrap();
    let pixel = pixmap.pixel(0, 0).unwrap().demultiply();
    let rgba = [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()];
    (rgba, (pixmap.width(), pixmap.height()))
}

#[test]
fn an_iconset_has_each_size_small_ones_drawn_small() {
    let folder = std::env::temp_dir().join(format!("file-types-{}.iconset", std::process::id()));
    write(&folder, &square("red"), &square("blue")).unwrap();
    let mut names: Vec<_> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let mut expected: Vec<_> = PNGS.iter().map(|(name, _)| (*name).to_owned()).collect();
    expected.sort();
    assert_eq!(names, expected);
    for (name, size) in PNGS {
        let (pixel, dimensions) = first_pixel(&folder.join(name));
        assert_eq!(dimensions, (size, size), "{name}");
        let color = if size <= SMALL {
            [0, 0, 255, 255]
        } else {
            [255, 0, 0, 255]
        };
        assert_eq!(pixel, color, "{name}");
    }
    fs::remove_dir_all(&folder).unwrap();
}
