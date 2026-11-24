//! A format's icon: a page in its kind's color, the kind's glyph on it in
//! white (`assets/icons/video.svg` or `music.svg`), and the extension at
//! the bottom in Fira, as outlines.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::Path;
use std::sync::Arc;

use file_types::Kind;
use resvg::usvg::{self, fontdb};

use crate::Result;

/// The icon's width and height.
pub const SIZE: f32 = 256.0;

const VIDEO: &str = include_str!("../../../../../assets/icons/video.svg");
const MUSIC: &str = include_str!("../../../../../assets/icons/music.svg");

/// The page's edges, and the size of its folded corner, top right.
const LEFT: f32 = 40.0;
const RIGHT: f32 = 216.0;
const TOP: f32 = 16.0;
const BOTTOM: f32 = 240.0;
const FOLD: f32 = 56.0;
const ROUND: f32 = 14.0;

/// The font the extensions are written in.
pub struct Labels {
    options: usvg::Options<'static>,
}

impl Labels {
    /// The labels in `font`, or else a Fira font of the system's: Fira
    /// Sans where there is one.
    pub fn find(font: Option<&Path>) -> Result<Self> {
        let mut fonts = fontdb::Database::new();
        let family = match font {
            Some(font) => {
                fonts
                    .load_font_file(font)
                    .map_err(|e| format!("{}: {e}", font.display()))?;
                families(&fonts).into_iter().next()
            }
            None => {
                fonts.load_system_fonts();
                fira(&fonts)
            }
        };
        let family = family.ok_or("no Fira font found: name one with --font FILE")?;
        let options = usvg::Options {
            fontdb: Arc::new(fonts),
            font_family: family,
            ..usvg::Options::default()
        };
        Ok(Self { options })
    }

    pub fn family(&self) -> &str {
        &self.options.font_family
    }

    /// The icon of a `kind` format, with `label` under the glyph, its
    /// letters as outlines, so the SVG needs no font.
    pub fn icon(&self, kind: Kind, label: Option<&str>) -> Result<String> {
        let svg = svg(kind, label, self.family())?;
        let tree = usvg::Tree::from_str(&svg, &self.options).map_err(|e| e.to_string())?;
        let svg = tree.to_string(&usvg::WriteOptions::default());
        // Drawn at any size, as an icon theme does.
        let size = format!(r#"width="{SIZE}" height="{SIZE}""#);
        let scalable = format!(r#"{size} viewBox="0 0 {SIZE} {SIZE}""#);
        Ok(svg.replacen(&size, &scalable, 1))
    }
}

/// The families of `fonts`, by name.
fn families(fonts: &fontdb::Database) -> BTreeSet<String> {
    let names = fonts.faces().flat_map(|face| &face.families);
    names.map(|(name, _)| name.clone()).collect()
}

/// Fira Sans, or the first family named Fira something.
fn fira(fonts: &fontdb::Database) -> Option<String> {
    let firas: BTreeSet<String> = families(fonts)
        .into_iter()
        .filter(|name| name.starts_with("Fira"))
        .collect();
    let sans = firas.get("Fira Sans").cloned();
    sans.or_else(|| firas.into_iter().next())
}

/// The icon as SVG, its label as text in `family`.
pub fn svg(kind: Kind, label: Option<&str>, family: &str) -> Result<String> {
    let (top, bottom) = match kind {
        Kind::Video => ("#9a6bff", "#6a3be0"),
        Kind::Audio => ("#ff5c6a", "#d92643"),
    };
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}" viewBox="0 0 {SIZE} {SIZE}">
<defs><linearGradient id="page" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/></linearGradient></defs>
<path d="{}" fill="url(#page)"/>
<path d="{}" fill="#fff" fill-opacity="0.4"/>
"##,
        page(),
        fold()
    );
    // Over the label, or bigger without one, as at the sizes it'd be too
    // small to read at.
    let (center, most) = if label.is_some() {
        (126.0, (90.0, 67.5))
    } else {
        (132.0, (102.0, 84.0))
    };
    svg.push_str(&glyph(kind, center, most)?);
    if let Some(label) = label {
        writeln!(
            svg,
            r##"<text x="128" y="208" text-anchor="middle" font-family="{family}" font-weight="700" font-size="38" fill="#fff">{label}</text>"##
        )
        .unwrap();
    }
    svg.push_str("</svg>\n");
    Ok(svg)
}

/// The page, rounded but for its folded corner.
fn page() -> String {
    format!(
        "M{l},{TOP} H{fold} L{RIGHT},{fold_bottom} V{b} Q{RIGHT},{BOTTOM} {r},{BOTTOM} \
         H{l} Q{LEFT},{BOTTOM} {LEFT},{b} V{t} Q{LEFT},{TOP} {l},{TOP} Z",
        l = LEFT + ROUND,
        r = RIGHT - ROUND,
        t = TOP + ROUND,
        b = BOTTOM - ROUND,
        fold = RIGHT - FOLD,
        fold_bottom = TOP + FOLD,
    )
}

/// The folded corner's flap.
fn fold() -> String {
    let (x, y, round) = (RIGHT - FOLD, TOP + FOLD, ROUND * 0.7);
    format!(
        "M{x},{TOP} V{} Q{x},{y} {},{y} H{RIGHT} Z",
        y - round,
        x + round
    )
}

/// The kind's glyph in white, as big as fits a box `most` wide and high
/// around (128, `center`).
fn glyph(kind: Kind, center: f32, most: (f32, f32)) -> Result<String> {
    let source = match kind {
        Kind::Video => VIDEO,
        Kind::Audio => MUSIC,
    };
    let bounds = usvg::Tree::from_str(source, &usvg::Options::default())
        .map_err(|e| e.to_string())?
        .root()
        .abs_stroke_bounding_box();
    let scale = (most.0 / bounds.width()).min(most.1 / bounds.height());
    let (width, height) = (bounds.width() * scale, bounds.height() * scale);
    let inner = source
        .split_once('>')
        .and_then(|(_, rest)| rest.rsplit_once("</svg>"))
        .map(|(inner, _)| inner.replace("#000", "#fff"))
        .ok_or("a glyph that isn't an <svg>")?;
    Ok(format!(
        r##"<svg x="{}" y="{}" width="{width}" height="{height}" viewBox="{} {} {} {}" fill="#fff">{inner}</svg>
"##,
        128.0 - width / 2.0,
        center - height / 2.0,
        bounds.x(),
        bounds.y(),
        bounds.width(),
        bounds.height()
    ))
}

#[cfg(test)]
#[path = "tests/icon.rs"]
mod tests;
