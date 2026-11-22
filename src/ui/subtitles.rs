//! Subtitles drawn over a block's picture: text centered near its bottom (or
//! top, or middle, if the cue says), sized to the picture, styled as the
//! settings say, and lifted above the controls while they show; bitmaps go
//! where the stream places them on the picture, and turn and crop with it.

use std::rc::Rc;
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

/// How the block around the subtitles is laid out, and how they look.
pub struct Layout {
    /// How the picture shows, which is fitted to the block.
    pub framing: Option<Framing>,
    /// The picture fills the block (cropped) rather than fitting in it.
    pub cover: bool,
    /// How tall the controls are at the bottom of the block.
    pub controls_height: Pixels,
    /// The controls show whether or not the mouse is over the block.
    pub controls_pinned: bool,
    pub mouse_in_window: bool,
    /// The font when the style doesn't name one.
    pub font: SharedString,
    pub style: SubtitleStyle,
}

/// An element covering the block that draws `cues`.
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
    lines: Vec<Rc<WrappedLine>>,
    /// The same lines in the edge color, if there is an edge.
    edges: Vec<Rc<WrappedLine>>,
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
    // Text is sized by the part of the picture that is in the block, and
    // goes on it or anywhere in the block.
    let visible = picture.intersect(&bounds);
    let area = match style.area {
        SubtitleArea::Picture => visible,
        SubtitleArea::Block => bounds,
    };
    let controls_showing = layout.controls_pinned
        || (layout.mouse_in_window && bounds.contains(&window.mouse_position()));
    let place = TextPlace {
        visible,
        area,
        controls_top: controls_showing.then(|| bounds.bottom() - layout.controls_height),
    };
    let family = family(&style, &layout.font);
    for mark in text_marks(cues, &style, &family, &place, window) {
        match mark {
            TextMark::Background {
                bounds,
                radius,
                color,
            } => window.paint_quad(fill(bounds, color).superellipse(radius)),
            TextMark::Line {
                line,
                area,
                line_height,
            } => {
                let _ = line.paint(
                    area.origin,
                    line_height,
                    TextAlign::Center,
                    Some(area),
                    window,
                    cx,
                );
            }
        }
    }
}

/// The font family `style` names, or `default` if it names none.
pub(crate) fn family(style: &SubtitleStyle, default: &SharedString) -> SharedString {
    if style.font.is_empty() {
        default.clone()
    } else {
        SharedString::from(style.font.clone())
    }
}

/// Where text subtitles go.
pub(crate) struct TextPlace {
    /// The part of the picture that shows, which the text is sized by.
    pub visible: Bounds<Pixels>,
    /// Where the text goes.
    pub area: Bounds<Pixels>,
    /// The top of the controls, while they show; text at the bottom goes
    /// above them.
    pub controls_top: Option<Pixels>,
}

/// What text subtitles draw, in order.
pub(crate) enum TextMark {
    /// A box behind a row of text.
    Background {
        bounds: Bounds<Pixels>,
        radius: Pixels,
        color: Hsla,
    },
    /// A line of text, wrapped into rows, each centered across `area`.
    Line {
        line: Rc<WrappedLine>,
        area: Bounds<Pixels>,
        line_height: Pixels,
    },
}

/// Lay out the text of `cues` in `place`, in `style` (clamped) and the
/// font `family`.
pub(crate) fn text_marks(
    cues: &[Arc<CueContent>],
    style: &SubtitleStyle,
    family: &SharedString,
    place: &TextPlace,
    window: &mut Window,
) -> Vec<TextMark> {
    let area = place.area;
    let (left, top) = (f32::from(area.left()), f32::from(area.top()));
    let (width, height) = (f32::from(area.size.width), f32::from(area.size.height));
    let font_size = (f32::from(place.visible.size.height) * FONT_SIZE)
        .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
        * style.size;
    let metrics = Metrics {
        font_size,
        line_height: font_size * LINE_HEIGHT,
        wrap_width: width * WRAP_WIDTH,
    };
    let text_color: Hsla = rgb(style.color.0).into();
    let edge_color = Hsla::from(rgb(style.edge_color.0)).opacity(EDGE_OPACITY);

    let blocks: Vec<Block> = cues
        .iter()
        .flat_map(|cue| &cue.texts)
        .map(|text| {
            let lines = shape(text, text_color, &metrics, family, style, window);
            let edges = match style.edge {
                SubtitleEdge::None => Vec::new(),
                SubtitleEdge::Outline | SubtitleEdge::Shadow => {
                    shape(text, edge_color, &metrics, family, style, window)
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
    let mut marks = Vec::new();
    if blocks.is_empty() {
        return marks;
    }

    let offset = (height * style.position).max(MIN_BOTTOM);
    let mut bottom = top + height - offset;
    if let Some(controls_top) = place.controls_top {
        let above_controls = controls_top - px(font_size * ABOVE_CONTROLS);
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
    let line_height = px(metrics.line_height);
    let line_mark = |line: &Rc<WrappedLine>, (x, y): (f32, f32)| TextMark::Line {
        line: line.clone(),
        area: Bounds {
            origin: point(px(x), px(y)),
            size: size(px(metrics.wrap_width), line.size(line_height).height),
        },
        line_height,
    };
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
                    marks.extend(backgrounds(line, x, y, &metrics, background));
                }
                if let Some(edge) = block.edges.get(i) {
                    for (dx, dy) in edge_offsets {
                        marks.push(line_mark(edge, (x + dx * edge_width, y + dy * edge_width)));
                    }
                }
                marks.push(line_mark(line, (x, y)));
                y += f32::from(line.size(line_height).height);
            }
        }
    }
    marks
}

/// A box behind each row a line of text wrapped into.
fn backgrounds(
    line: &WrappedLine,
    x: f32,
    y: f32,
    metrics: &Metrics,
    color: Hsla,
) -> impl Iterator<Item = TextMark> {
    let padding = metrics.font_size * BACKGROUND_PADDING;
    let (wrap_width, line_height) = (metrics.wrap_width, metrics.line_height);
    let radius = px(metrics.font_size * BACKGROUND_RADIUS);
    row_widths(line)
        .into_iter()
        .enumerate()
        .map(move |(row, width)| TextMark::Background {
            bounds: Bounds {
                origin: point(
                    px(x + (wrap_width - width) / 2.0 - padding),
                    px(y + row as f32 * line_height),
                ),
                size: size(px(width + 2.0 * padding), px(line_height)),
            },
            radius,
            color,
        })
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
) -> Vec<Rc<WrappedLine>> {
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
        .map(|lines| lines.into_iter().map(Rc::new).collect())
        .unwrap_or_default()
}
