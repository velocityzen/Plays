//! Lines of an ASS or SSA track laid out as its script has them: in its
//! fonts, sizes and colors, at their alignments within their margins or
//! where `\pos` puts them, with outlines, shadows or boxes as wide as it
//! says, all at the size the picture shows the script's canvas at. Lines
//! placed by their alignment stack, the first nearest its edge, and go in
//! the part of the picture that shows; those placed by `\pos` go where it
//! says on the picture, wherever it is.

use std::f32::consts::TAU;
use std::rc::Rc;

use gpui::{
    Bounds, Font, FontStyle, FontWeight, Hsla, Pixels, SharedString, TextAlign, TextRun, Window,
    WrappedLine, point, px, rgba, size,
};

use super::subtitles::{TextMark, TextPlace, row_widths};
use super::theme::subtitles::{
    ABOVE_CONTROLS, MIN_WRAP, OUTLINE_RING, OUTLINE_RINGS, RULE_WIDTH, STRIKEOUT_HEIGHT,
    UNDERLINE_DEPTH,
};
use crate::media::subtitles::{Look, Span, Styled, Text};

/// How a span looks when its script doesn't say: as libass's own style.
const PLAIN: Look = Look {
    fill: 0xffff_ffff,
    secondary: 0x00ff_ffff,
    outline: 0x0000_00ff,
    shadow: 0x0000_007f,
    underline: false,
    strikeout: false,
    karaoke: None,
};

/// A line shaped and sized for painting, and where it goes.
struct Line<'a> {
    text: &'a Text,
    styled: &'a Styled,
    /// Its parts between line breaks, each wrapped into rows: in their
    /// fill colors, and in their outline and shadow colors if it has
    /// outlines and shadows drawn as text.
    fills: Vec<Rc<WrappedLine>>,
    outlines: Vec<Rc<WrappedLine>>,
    shadows: Vec<Rc<WrappedLine>>,
    /// Where in `text` each part starts, in bytes.
    starts: Vec<usize>,
    /// How many pixels a unit of the script is, across and down.
    scale: (f32, f32),
    font_size: f32,
    line_height: f32,
    wrap_width: f32,
    height: f32,
    outline: f32,
    shadow: f32,
    /// The left of where it wraps, and its top.
    origin: (f32, f32),
}

/// Lay out `texts`, sized `size` times as their scripts say, in their
/// own fonts or else `family`.
pub(crate) fn styled_marks(
    texts: &[&Text],
    size: f32,
    family: &SharedString,
    place: &TextPlace,
    window: &mut Window,
) -> Vec<TextMark> {
    let mut lines: Vec<Line> = texts
        .iter()
        .filter_map(|text| shape(text, size, family, place, window))
        .collect();
    place_lines(&mut lines, place);
    lines.iter().flat_map(marks).collect()
}

/// How many pixels a unit of `styled`'s script is, across and down: the
/// decoded picture is its canvas.
fn script_scale(styled: &Styled, place: &TextPlace) -> (f32, f32) {
    let picture = place.picture;
    let (width, height) = place.framing.map_or(
        (
            f32::from(picture.size.width),
            f32::from(picture.size.height),
        ),
        |framing| framing.decoded(picture),
    );
    let (canvas_width, canvas_height) = styled.canvas;
    (
        width / canvas_width.max(1.0),
        height / canvas_height.max(1.0),
    )
}

/// Where a point of `styled`'s script shows, turned and cropped with the
/// picture.
fn on_picture(styled: &Styled, place: &TextPlace, (x, y): (f32, f32)) -> (f32, f32) {
    let (canvas_width, canvas_height) = styled.canvas;
    let at = [
        x / canvas_width.max(1.0),
        y / canvas_height.max(1.0),
        0.0,
        0.0,
    ];
    let [x, y, ..] = place.framing.map_or(at, |framing| framing.place(at).0);
    let picture = place.picture;
    (
        f32::from(picture.left()) + f32::from(picture.size.width) * x,
        f32::from(picture.top()) + f32::from(picture.size.height) * y,
    )
}

/// The font `styled` names, if it's installed, else `family`.
fn family_of(styled: &Styled, family: &SharedString, window: &Window) -> SharedString {
    let named = SharedString::from(styled.font.to_string());
    let installed = !named.is_empty()
        && window
            .text_system()
            .font_id(&gpui::font(named.clone()))
            .is_ok();
    if installed { named } else { family.clone() }
}

fn shape<'a>(
    text: &'a Text,
    size: f32,
    family: &SharedString,
    place: &TextPlace,
    window: &mut Window,
) -> Option<Line<'a>> {
    let styled = text.styled.as_ref()?;
    let scale = script_scale(styled, place);
    let (across, down) = scale;
    let family = family_of(styled, family, window);
    // A script's font size is the height of a line, ascent and descent.
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&gpui::font(family.clone()));
    let cell = f32::from(text_system.ascent(font_id, px(1.)))
        + f32::from(text_system.descent(font_id, px(1.))).abs();
    // No taller than the picture, whatever the script says.
    let line_height = (styled.size * down * size).min(f32::from(place.visible.size.height));
    if line_height < 1.0 {
        return None;
    }
    let font_size = line_height / cell.max(0.5);
    let visible = f32::from(place.visible.size.width);
    let [left, right, _] = styled.margins;
    let wrap_width = match styled.position {
        Some(_) => (styled.canvas.0 - left - right) * across,
        None => visible - (left + right) * across,
    }
    .max(visible * MIN_WRAP);
    let shaped = |color: fn(&Look) -> u32, window: &mut Window| {
        let runs = runs(text, &family, color);
        window
            .text_system()
            .shape_text(
                text.text.clone().into(),
                px(font_size),
                &runs,
                Some(px(wrap_width)),
                None,
            )
            .map(|lines| lines.into_iter().map(Rc::new).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let (outline, shadow) = (styled.outline * down, styled.shadow * down);
    let fills = shaped(|look| look.fill, window);
    let outlines = if outline > 0.0 && !styled.boxed {
        shaped(|look| look.outline, window)
    } else {
        Vec::new()
    };
    let shadows = if shadow > 0.0 && !styled.boxed {
        shaped(|look| look.shadow, window)
    } else {
        Vec::new()
    };
    let mut starts = vec![0];
    starts.extend(text.text.match_indices('\n').map(|(at, _)| at + 1));
    let height = fills
        .iter()
        .map(|line| f32::from(line.size(px(line_height)).height))
        .sum();
    Some(Line {
        text,
        styled,
        fills,
        outlines,
        shadows,
        starts,
        scale,
        font_size,
        line_height,
        wrap_width,
        height,
        outline,
        shadow,
        origin: (0.0, 0.0),
    })
}

/// A run for each span of `text`, in the font `family` and the color of
/// its look `color` picks.
fn runs(text: &Text, family: &SharedString, color: fn(&Look) -> u32) -> Vec<TextRun> {
    text.spans
        .iter()
        .map(|span| TextRun {
            len: span.len,
            font: Font {
                weight: if span.bold {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
                style: if span.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
                ..gpui::font(family.clone())
            },
            color: rgba(color(&span.look.unwrap_or(PLAIN))).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect()
}

/// Put each line where its alignment and margins, or `\pos`, say.
fn place_lines(lines: &mut [Line], place: &TextPlace) {
    let visible = place.visible;
    let (left, top) = (f32::from(visible.left()), f32::from(visible.top()));
    let (width, height) = (
        f32::from(visible.size.width),
        f32::from(visible.size.height),
    );
    let row = |line: &Line| (line.styled.alignment.clamp(1, 9) - 1) / 3;
    let middle: f32 = lines
        .iter()
        .filter(|line| line.styled.position.is_none() && row(line) == 1)
        .map(|line| line.height)
        .sum();
    // How far up lines along the bottom have stacked, and down along the
    // top, and on in the middle.
    let mut floor = f32::INFINITY;
    let mut ceiling = f32::NEG_INFINITY;
    let mut middle = top + (height - middle) / 2.0;
    for line in lines {
        let styled = line.styled;
        let (across, down) = line.scale;
        let [margin_left, margin_right, margin_vertical] = styled.margins;
        let (wrap, tall) = (line.wrap_width, line.height);
        let x = match styled.position {
            Some(at) => {
                let (x, _) = on_picture(styled, place, at);
                match align(styled.alignment) {
                    TextAlign::Left => x,
                    TextAlign::Center => x - wrap / 2.0,
                    TextAlign::Right => x - wrap,
                }
            }
            None if wrap > width - (margin_left + margin_right) * across => {
                left + (width - wrap) / 2.0
            }
            None => left + margin_left * across,
        };
        let y = match (styled.position, row(line)) {
            (Some(at), row) => {
                let (_, y) = on_picture(styled, place, at);
                match row {
                    0 => y - tall,
                    1 => y - tall / 2.0,
                    _ => y,
                }
            }
            (None, 0) => {
                let lifted = place.controls_top.map_or(f32::INFINITY, |controls| {
                    f32::from(controls) - line.font_size * ABOVE_CONTROLS
                });
                let bottom = (top + height - margin_vertical * down)
                    .min(floor)
                    .min(lifted);
                floor = bottom - tall;
                floor
            }
            (None, 1) => {
                middle += tall;
                middle - tall
            }
            (None, _) => {
                let y = (top + margin_vertical * down).max(ceiling);
                ceiling = y + tall;
                y
            }
        };
        line.origin = (x, y);
    }
}

/// How rows line up across where they wrap, as a numeric keypad's columns.
fn align(alignment: u8) -> TextAlign {
    match alignment % 3 {
        1 => TextAlign::Left,
        2 => TextAlign::Center,
        _ => TextAlign::Right,
    }
}

/// The left of each row `line` wraps into, and its width, lined up across
/// `width` from `x` as `align` says, as gpui lines them up.
fn rows(line: &WrappedLine, x: f32, width: f32, align: TextAlign) -> Vec<(f32, f32)> {
    row_widths(line)
        .into_iter()
        .map(|row| match align {
            TextAlign::Left => (x, row),
            TextAlign::Center => (x + (width - row) / 2.0, row),
            TextAlign::Right => (x + width - row, row),
        })
        .collect()
}

/// Copies of a text drawn around itself, `width` from it at most, in
/// rings close enough together that nothing shows between them.
fn ring(width: f32) -> Vec<(f32, f32)> {
    if width <= 0.0 {
        return Vec::new();
    }
    let rings = (width / OUTLINE_RING).ceil().clamp(1.0, OUTLINE_RINGS);
    let mut offsets = Vec::new();
    for ring in 1..=rings as usize {
        let radius = width * ring as f32 / rings;
        let steps = if radius <= 2.0 { 8 } else { 16 };
        offsets.extend((0..steps).map(|step| {
            let angle = TAU * step as f32 / steps as f32;
            (radius * angle.cos(), radius * angle.sin())
        }));
    }
    offsets
}

/// What a line draws, back to front: its shadow, its outline or box, its
/// text, and its underlines and strikethroughs.
fn marks(line: &Line) -> Vec<TextMark> {
    let align = align(line.styled.alignment);
    let line_height = px(line.line_height);
    let (x, mut y) = line.origin;
    // Each part's top, and its rows.
    let parts: Vec<(f32, Vec<(f32, f32)>)> = line
        .fills
        .iter()
        .map(|fill| {
            let top = y;
            y += f32::from(fill.size(line_height).height);
            (top, rows(fill, x, line.wrap_width, align))
        })
        .collect();
    let text = |wrapped: &Rc<WrappedLine>, top: f32, (dx, dy): (f32, f32)| {
        let tall = wrapped.size(line_height).height;
        TextMark::Line {
            line: wrapped.clone(),
            area: Bounds {
                origin: point(px(x + dx), px(top + dy)),
                size: size(px(line.wrap_width), tall),
            },
            line_height,
            align,
        }
    };
    let mut marks = Vec::new();
    let first = line
        .text
        .spans
        .first()
        .and_then(|span| span.look)
        .unwrap_or(PLAIN);
    let shadowed = (line.shadow, line.shadow);
    if line.styled.boxed {
        let boxes = boxes(line, &parts);
        if line.shadow > 0.0 {
            marks.extend(boxes.iter().map(|&bounds| TextMark::Background {
                bounds: bounds + point(px(shadowed.0), px(shadowed.1)),
                radius: px(0.),
                color: rgba(first.shadow).into(),
            }));
        }
        marks.extend(boxes.into_iter().map(|bounds| TextMark::Background {
            bounds,
            radius: px(0.),
            color: rgba(first.outline).into(),
        }));
    } else {
        // Translucent shadows are drawn once, so they don't darken where
        // copies would overlap.
        let opaque = line
            .text
            .spans
            .iter()
            .all(|span| span.look.unwrap_or(PLAIN).shadow & 0xff == 0xff);
        let mut around = vec![(0.0, 0.0)];
        if opaque {
            around.extend(ring(line.outline));
        }
        for (shadow, &(top, _)) in line.shadows.iter().zip(&parts) {
            for &(dx, dy) in &around {
                marks.push(text(shadow, top, (dx + shadowed.0, dy + shadowed.1)));
            }
        }
        let ring = ring(line.outline);
        for (outline, &(top, _)) in line.outlines.iter().zip(&parts) {
            marks.extend(ring.iter().map(|&offset| text(outline, top, offset)));
        }
    }
    for ((fill, (top, rows)), start) in line.fills.iter().zip(&parts).zip(&line.starts) {
        marks.push(text(fill, *top, (0.0, 0.0)));
        let spans = (&line.text.spans[..], *start);
        marks.extend(rules(fill, spans, rows, *top, line.line_height));
    }
    marks
}

/// A box behind each row of `line`'s `parts`, as far out as its outline is
/// wide; only the first and last reach out above and below.
fn boxes(line: &Line, parts: &[(f32, Vec<(f32, f32)>)]) -> Vec<Bounds<Pixels>> {
    let pad = line.outline;
    let rows: Vec<(f32, f32, f32)> = parts
        .iter()
        .flat_map(|(top, rows)| {
            rows.iter()
                .enumerate()
                .map(move |(at, &(left, width))| (left, top + at as f32 * line.line_height, width))
        })
        .collect();
    let last = rows.len().saturating_sub(1);
    rows.iter()
        .enumerate()
        .map(|(at, &(left, top, width))| {
            let above = if at == 0 { pad } else { 0.0 };
            let below = if at == last { pad } else { 0.0 };
            Bounds {
                origin: point(px(left - pad), px(top - above)),
                size: size(px(width + 2.0 * pad), px(line.line_height + above + below)),
            }
        })
        .collect()
}

/// The underlines and strikethroughs of a part of a line: of its glyphs
/// in spans that have them, in their fill color, across its `rows` from
/// `top`. `spans` are the line's, and where in it the part starts.
fn rules(
    part: &WrappedLine,
    (spans, start): (&[Span], usize),
    rows: &[(f32, f32)],
    top: f32,
    line_height: f32,
) -> Vec<TextMark> {
    let layout = &part.unwrapped_layout;
    let (ascent, descent) = (f32::from(layout.ascent), f32::from(layout.descent));
    let baseline = (line_height - ascent - descent) / 2.0 + ascent;
    let thickness = (f32::from(layout.font_size) * RULE_WIDTH).max(1.0);
    // Each glyph's row, where it starts in it, and how its span looks.
    let mut glyphs: Vec<(usize, f32, Look)> = Vec::new();
    let mut boundaries = part.wrap_boundaries().iter().peekable();
    let (mut row, mut row_start) = (0, 0.0);
    let mut spans = spans.iter().scan(0, |end, span| {
        *end += span.len;
        Some((*end, span.look.unwrap_or(PLAIN)))
    });
    let mut span = spans.next();
    for (run_ix, run) in layout.runs.iter().enumerate() {
        for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
            if boundaries
                .next_if(|wrap| (wrap.run_ix, wrap.glyph_ix) == (run_ix, glyph_ix))
                .is_some()
            {
                row += 1;
                row_start = f32::from(glyph.position.x);
            }
            while span.is_some_and(|(end, _)| end <= start + glyph.index) {
                span = spans.next();
            }
            let look = span.map_or(PLAIN, |(_, look)| look);
            glyphs.push((row, f32::from(glyph.position.x) - row_start, look));
        }
    }
    // Runs of glyphs in a row that look the same: their row, look, and
    // where they start and end in it.
    let mut runs: Vec<(usize, Look, f32, f32)> = Vec::new();
    for (at, &(row, x, look)) in glyphs.iter().enumerate() {
        let end = match glyphs.get(at + 1) {
            Some(&(next, x, _)) if next == row => x,
            _ => rows.get(row).map_or(x, |&(_, width)| width),
        };
        match runs.last_mut() {
            Some(run) if (run.0, run.1) == (row, look) => run.3 = end,
            _ => runs.push((row, look, x, end)),
        }
    }
    let mut marks = Vec::new();
    for (row, look, start, end) in runs {
        let Some(&(left, _)) = rows.get(row) else {
            continue;
        };
        let row_top = top + row as f32 * line_height;
        let lines = [
            (look.underline, baseline + descent * UNDERLINE_DEPTH),
            (look.strikeout, baseline - ascent * STRIKEOUT_HEIGHT),
        ];
        for (_, depth) in lines.into_iter().filter(|(on, _)| *on) {
            marks.push(TextMark::Background {
                bounds: Bounds {
                    origin: point(px(left + start), px(row_top + depth - thickness / 2.0)),
                    size: size(px((end - start).max(0.0)), px(thickness)),
                },
                radius: px(0.),
                color: Hsla::from(rgba(look.fill)),
            });
        }
    }
    marks
}

#[cfg(test)]
#[path = "tests/ass.rs"]
mod tests;
