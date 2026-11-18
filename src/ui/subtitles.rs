//! Subtitles drawn over a tile's picture: text centered near its bottom (or
//! top, or middle, if the cue says), sized to the picture, styled as the
//! settings say, and lifted above the controls while they show; bitmaps go
//! where the stream places them on the picture, and turn and crop with it.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ContentMask, ContentTransform, Corners, Font, FontStyle, FontWeight, Hsla, Pixels,
    SharedString, TextAlign, TextRun, Window, WrappedLine, canvas, fill, point, px, rgb, size,
};

use super::framing::Framing;
use super::theme::subtitles::{
    ABOVE_CONTROLS, BACKGROUND_PADDING, BACKGROUND_RADIUS, EDGE_OPACITY, EDGE_WIDTH, FONT_SIZE,
    LINE_HEIGHT, MAX_FONT_SIZE, MIN_BOTTOM, MIN_FONT_SIZE, SHADOW_OFFSET, WRAP_WIDTH,
};
use crate::media::subtitles::{CueContent, Placement, Text};
use crate::settings::{SubtitleArea, SubtitleEdge, SubtitleStyle};

/// How the tile around the subtitles is laid out, and how they look.
pub struct Layout {
    /// How the picture shows, which is fitted to the tile.
    pub framing: Option<Framing>,
    /// The picture fills the tile (cropped) rather than fitting in it.
    pub cover: bool,
    /// How tall the controls are at the bottom of the tile.
    pub controls_height: Pixels,
    /// The controls show whether or not the mouse is over the tile.
    pub controls_pinned: bool,
    pub mouse_in_window: bool,
    /// The font when the style doesn't name one.
    pub font: SharedString,
    pub style: SubtitleStyle,
}

/// An element covering the tile that draws `cues`.
pub fn overlay(cues: Vec<Arc<CueContent>>, layout: Layout) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, cx| paint(&cues, &layout, bounds, window, cx),
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// An outline is the text drawn in the edge color all around itself first.
const OUTLINE: [(f32, f32); 8] = [
    (-1.0, 0.0),
    (1.0, 0.0),
    (0.0, -1.0),
    (0.0, 1.0),
    (-0.7, -0.7),
    (0.7, -0.7),
    (-0.7, 0.7),
    (0.7, 0.7),
];

/// A shadow is the text drawn in the edge color below and to the right.
const SHADOW: [(f32, f32); 1] = [(SHADOW_OFFSET, SHADOW_OFFSET)];

/// A cue's text, shaped for painting.
struct Block {
    placement: Placement,
    lines: Vec<WrappedLine>,
    /// The same lines in the edge color, if there is an edge.
    edges: Vec<WrappedLine>,
    height: f32,
}

/// The sizes text is drawn at.
struct Metrics {
    font_size: f32,
    line_height: f32,
    wrap_width: f32,
}

pub(crate) fn paint(
    cues: &[Arc<CueContent>],
    layout: &Layout,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let picture = layout
        .framing
        .map_or(bounds, |framing| framing.fit(bounds, layout.cover));
    // Cropped away with the picture.
    window.with_content_mask(Some(ContentMask { bounds: picture }), |window| {
        for bitmap in cues.iter().flat_map(|cue| &cue.bitmaps) {
            let ([x, y, width, height], content) = layout
                .framing
                .map_or((bitmap.rect, ContentTransform::IDENTITY), |framing| {
                    framing.place(bitmap.rect)
                });
            let rect = Bounds {
                origin: point(
                    picture.origin.x + picture.size.width * x,
                    picture.origin.y + picture.size.height * y,
                ),
                size: size(picture.size.width * width, picture.size.height * height),
            };
            let image = bitmap.image.clone();
            let _ = window.paint_image_with_content(
                rect,
                Corners::default(),
                0.,
                image,
                0,
                false,
                content,
            );
        }
    });

    let style = layout.style.clamped();
    // Text is sized by the part of the picture that is in the tile, and
    // goes on it or anywhere in the tile.
    let visible = picture.intersect(&bounds);
    let area = match style.area {
        SubtitleArea::Picture => visible,
        SubtitleArea::Tile => bounds,
    };
    let (left, top) = (f32::from(area.left()), f32::from(area.top()));
    let (width, height) = (f32::from(area.size.width), f32::from(area.size.height));
    let font_size = (f32::from(visible.size.height) * FONT_SIZE)
        .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
        * style.size;
    let metrics = Metrics {
        font_size,
        line_height: font_size * LINE_HEIGHT,
        wrap_width: width * WRAP_WIDTH,
    };
    let family = if style.font.is_empty() {
        layout.font.clone()
    } else {
        SharedString::from(style.font.clone())
    };
    let text_color: Hsla = rgb(style.color.0).into();
    let edge_color = Hsla::from(rgb(style.edge_color.0)).opacity(EDGE_OPACITY);

    let blocks: Vec<Block> = cues
        .iter()
        .flat_map(|cue| &cue.texts)
        .map(|text| {
            let lines = shape(text, text_color, &metrics, &family, &style, window);
            let edges = match style.edge {
                SubtitleEdge::None => Vec::new(),
                SubtitleEdge::Outline | SubtitleEdge::Shadow => {
                    shape(text, edge_color, &metrics, &family, &style, window)
                }
            };
            let height = lines
                .iter()
                .map(|line| f32::from(line.size(px(metrics.line_height)).height))
                .sum();
            Block {
                placement: text.placement,
                lines,
                edges,
                height,
            }
        })
        .collect();
    if blocks.is_empty() {
        return;
    }

    let offset = (height * style.position).max(MIN_BOTTOM);
    let mut bottom = top + height - offset;
    let controls_showing = layout.controls_pinned
        || (layout.mouse_in_window && bounds.contains(&window.mouse_position()));
    if controls_showing {
        let above_controls =
            bounds.bottom() - layout.controls_height - px(font_size * ABOVE_CONTROLS);
        bottom = bottom.min(f32::from(above_controls));
    }

    let x = left + (width - metrics.wrap_width) / 2.0;
    let edge_offsets: &[(f32, f32)] = match style.edge {
        SubtitleEdge::None => &[],
        SubtitleEdge::Outline => &OUTLINE,
        SubtitleEdge::Shadow => &SHADOW,
    };
    let edge_width = (font_size * EDGE_WIDTH).max(1.0);
    let background = Hsla::from(rgb(style.background.0)).opacity(style.background_opacity);
    for placement in [Placement::Top, Placement::Middle, Placement::Bottom] {
        let blocks: Vec<&Block> = blocks.iter().filter(|b| b.placement == placement).collect();
        let total: f32 = blocks.iter().map(|b| b.height).sum();
        let mut y = match placement {
            Placement::Top => top + offset,
            Placement::Middle => top + (height - total) / 2.0,
            Placement::Bottom => bottom - total,
        };
        for block in blocks {
            for (i, line) in block.lines.iter().enumerate() {
                if style.background_opacity > 0.0 {
                    paint_background(line, x, y, &metrics, background, window);
                }
                if let Some(edge) = block.edges.get(i) {
                    for (dx, dy) in edge_offsets {
                        let at = (x + dx * edge_width, y + dy * edge_width);
                        paint_line(edge, at, &metrics, window, cx);
                    }
                }
                paint_line(line, (x, y), &metrics, window, cx);
                y += f32::from(line.size(px(metrics.line_height)).height);
            }
        }
    }
}

/// Paint a line of text centered in the wrap width from `(x, y)`.
fn paint_line(
    line: &WrappedLine,
    (x, y): (f32, f32),
    metrics: &Metrics,
    window: &mut Window,
    cx: &mut App,
) {
    let line_height = px(metrics.line_height);
    let area = Bounds {
        origin: point(px(x), px(y)),
        size: size(px(metrics.wrap_width), line.size(line_height).height),
    };
    let _ = line.paint(
        area.origin,
        line_height,
        TextAlign::Center,
        Some(area),
        window,
        cx,
    );
}

/// A box behind each row a line of text wrapped into.
fn paint_background(
    line: &WrappedLine,
    x: f32,
    y: f32,
    metrics: &Metrics,
    color: Hsla,
    window: &mut Window,
) {
    let padding = metrics.font_size * BACKGROUND_PADDING;
    for (row, width) in row_widths(line).into_iter().enumerate() {
        let rect = Bounds {
            origin: point(
                px(x + (metrics.wrap_width - width) / 2.0 - padding),
                px(y + row as f32 * metrics.line_height),
            ),
            size: size(px(width + 2.0 * padding), px(metrics.line_height)),
        };
        window
            .paint_quad(fill(rect, color).superellipse(px(metrics.font_size * BACKGROUND_RADIUS)));
    }
}

/// How wide each row of a wrapped line is, measured as gpui centers them:
/// from the first glyph of a row to the first glyph of the next.
fn row_widths(line: &WrappedLine) -> Vec<f32> {
    let layout = &line.unwrapped_layout;
    let mut edges = vec![0.0];
    for boundary in line.wrap_boundaries() {
        let glyph = layout
            .runs
            .get(boundary.run_ix)
            .and_then(|run| run.glyphs.get(boundary.glyph_ix));
        if let Some(glyph) = glyph {
            edges.push(f32::from(glyph.position.x));
        }
    }
    edges.push(f32::from(layout.width));
    edges
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).max(0.0))
        .collect()
}

fn shape(
    text: &Text,
    color: Hsla,
    metrics: &Metrics,
    family: &SharedString,
    style: &SubtitleStyle,
    window: &mut Window,
) -> Vec<WrappedLine> {
    let weight = style.weight.value();
    let runs: Vec<TextRun> = text
        .spans
        .iter()
        .map(|span| TextRun {
            len: span.len,
            font: Font {
                weight: FontWeight(if span.bold {
                    (weight + 300.0).min(900.0)
                } else {
                    weight
                }),
                style: if span.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
                ..gpui::font(family.clone())
            },
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect();
    window
        .text_system()
        .shape_text(
            text.text.clone().into(),
            px(metrics.font_size),
            &runs,
            Some(px(metrics.wrap_width)),
            None,
        )
        .map(|lines| lines.into_vec())
        .unwrap_or_default()
}
