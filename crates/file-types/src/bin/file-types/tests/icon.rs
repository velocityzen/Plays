use super::*;

/// The SVG `svg` read without fonts.
fn read(svg: &str) -> usvg::Tree {
    usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap()
}

#[test]
fn a_glyph_is_on_the_page_above_the_label() {
    for kind in [Kind::Video, Kind::Audio] {
        for (label, below) in [(None, BOTTOM), (Some("WEBM"), 170.0)] {
            let tree = read(&svg(kind, label, "Fira Sans").unwrap());
            // The page, its fold, and the glyph; the label needs a font.
            let parts = tree.root().children();
            assert_eq!(parts.len(), 3, "{kind:?} {label:?}");
            let glyph = parts[2].abs_stroke_bounding_box();
            assert!(
                glyph.left() > LEFT && glyph.right() < RIGHT,
                "{kind:?} {glyph:?}"
            );
            assert!(
                glyph.top() > TOP + 20.0 && glyph.bottom() < below,
                "{kind:?} {glyph:?}"
            );
            let middle = f32::midpoint(glyph.left(), glyph.right());
            assert!((middle - SIZE / 2.0).abs() < 0.01, "{kind:?} {glyph:?}");
        }
    }
}

#[test]
fn the_label_is_in_the_family_at_the_bottom() {
    let svg = svg(Kind::Audio, Some("FLAC"), "Fira Code").unwrap();
    let text = svg.lines().find(|line| line.starts_with("<text")).unwrap();
    assert!(text.contains(r#"font-family="Fira Code""#), "{text}");
    assert!(text.ends_with(">FLAC</text>"), "{text}");
}

#[test]
fn a_glyph_is_white() {
    for kind in [Kind::Video, Kind::Audio] {
        let svg = svg(kind, None, "Fira Sans").unwrap();
        let tree = read(&svg);
        let mut painted = Vec::new();
        paints(tree.root(), &mut painted);
        // The page's gradient, the fold's white, and the glyph's white.
        assert!(painted.len() >= 3, "{kind:?} {painted:?}");
        assert!(
            painted[2..].iter().all(|p| *p == "#ffffff"),
            "{kind:?} {painted:?}"
        );
    }
}

/// The colors the paths in `group` are filled or stroked with, in order.
fn paints(group: &usvg::Group, painted: &mut Vec<String>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => paints(group, painted),
            usvg::Node::Path(path) => {
                let fill = path.fill().map(usvg::Fill::paint);
                let stroke = path.stroke().map(usvg::Stroke::paint);
                for paint in fill.into_iter().chain(stroke) {
                    painted.push(match paint {
                        usvg::Paint::Color(c) => {
                            format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue)
                        }
                        _ => "a gradient".to_owned(),
                    });
                }
            }
            _ => {}
        }
    }
}
