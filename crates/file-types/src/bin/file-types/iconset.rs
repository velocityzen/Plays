//! An icon as an `.iconset`, the folder of PNGs `iconutil` makes an
//! `.icns` of: the icon at each size Finder draws, at one and two pixels
//! a point.

use std::fs;
use std::path::Path;

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

use crate::{Result, at};

/// Each PNG's name in an iconset, and its width and height in pixels.
pub const PNGS: [(&str, u32); 10] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

/// Up to how many pixels wide an icon is drawn as `small` instead, as
/// its label couldn't be read.
pub const SMALL: u32 = 32;

/// Writes the iconset of the icon `svg` to `folder`, drawn as `small` up
/// to `SMALL` pixels.
pub fn write(folder: &Path, svg: &str, small: &str) -> Result<()> {
    let (svg, small) = (tree(svg)?, tree(small)?);
    fs::create_dir_all(folder).map_err(at(folder))?;
    for (name, size) in PNGS {
        let tree = if size <= SMALL { &small } else { &svg };
        let path = folder.join(name);
        fs::write(&path, png(tree, size)?).map_err(at(&path))?;
    }
    Ok(())
}

fn tree(svg: &str) -> Result<usvg::Tree> {
    usvg::Tree::from_str(svg, &usvg::Options::default()).map_err(|e| e.to_string())
}

/// `tree` drawn `size` pixels wide and high, as PNG.
pub fn png(tree: &usvg::Tree, size: u32) -> Result<Vec<u8>> {
    let mut pixmap = Pixmap::new(size, size).ok_or("an icon with no pixels")?;
    let scale = size as f32 / tree.size().width();
    resvg::render(
        tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "tests/iconset.rs"]
mod tests;
