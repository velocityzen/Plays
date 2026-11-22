//! Subtitles drawn into pixels by Plays rather than by gpui: into
//! screenshots, and into the frames Picture in Picture shows. Text is laid
//! out as on a tile showing just the picture, `TEXT_HEIGHT` pixels tall,
//! then drawn at the size of the pixels with the glyphs gpui draws on
//! screen; bitmaps are stretched over where the stream places them.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    Bounds, ContentTransform, FontId, GlyphId, GlyphRaster, Hsla, LineGlyph, Pixels, RenderImage,
    Rgba, SharedString, TextAlign, TextSystem, Window, point, px, size,
};

use super::subtitles::{self, TextMark, TextPlace};
use super::theme::subtitles::TEXT_HEIGHT;
use crate::media::subtitles::CueContent;
use crate::settings::SubtitleStyle;

/// Something text subtitles draw.
pub enum Paint {
    /// A rounded box behind a row of text.
    Box {
        bounds: Bounds<Pixels>,
        radius: Pixels,
        color: Hsla,
    },
    Glyph(LineGlyph),
}

/// Lay out the text of `cues` for a picture shown at `picture` size, as
/// on a tile showing just that picture, `TEXT_HEIGHT` pixels tall.
pub fn text_overlay(
    cues: &[Arc<CueContent>],
    style: &SubtitleStyle,
    font: &SharedString,
    picture: (f32, f32),
    window: &mut Window,
) -> Vec<Paint> {
    let (width, height) = picture;
    if width <= 0.0 || height <= 0.0 || cues.iter().all(|cue| cue.texts.is_empty()) {
        return Vec::new();
    }
    let bounds = Bounds {
        origin: point(px(0.), px(0.)),
        size: size(px(TEXT_HEIGHT * width / height), px(TEXT_HEIGHT)),
    };
    let style = style.clamped();
    let place = TextPlace {
        visible: bounds,
        area: bounds,
        controls_top: None,
    };
    let family = subtitles::family(&style, font);
    let mut paints = Vec::new();
    for mark in subtitles::text_marks(cues, &style, &family, &place, window) {
        match mark {
            TextMark::Background {
                bounds,
                radius,
                color,
            } => paints.push(Paint::Box {
                bounds,
                radius,
                color,
            }),
            TextMark::Line {
                line,
                area,
                line_height,
            } => paints.extend(
                line.glyphs(area.origin, line_height, TextAlign::Center, Some(area))
                    .into_iter()
                    .map(Paint::Glyph),
            ),
        }
    }
    paints
}

/// Pixels subtitles can be drawn into.
pub trait Target {
    /// Width and height, in pixels.
    fn size(&self) -> (usize, usize);

    /// An sRGB color in these pixels' signal.
    fn signal(&self, srgb: [f32; 3]) -> [f32; 3];

    /// Lay `signal` over the pixel at `x`, `y` with `alpha`.
    fn blend(&mut self, x: usize, y: usize, signal: [f32; 3], alpha: f32);
}

/// Draw a BGRA image (straight alpha) stretched over `rect`, in fractions
/// of the target, sampled the way `sampling` says (as `Framing::place`
/// gives it for a turned picture).
pub fn draw_image(
    target: &mut impl Target,
    image: &RenderImage,
    rect: [f32; 4],
    sampling: ContentTransform,
) {
    let (Some(bytes), image_size) = (image.as_bytes(0), image.size(0)) else {
        return;
    };
    let (image_width, image_height) = (image_size.width.0 as usize, image_size.height.0 as usize);
    if image_width == 0 || image_height == 0 || bytes.len() < image_width * image_height * 4 {
        return;
    }
    let (target_width, target_height) = target.size();
    let [x, y, width, height] = rect;
    let (left, top) = (x * target_width as f32, y * target_height as f32);
    let (width, height) = (width * target_width as f32, height * target_height as f32);
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let ContentTransform {
        origin,
        across,
        down,
    } = sampling;
    let columns = span(left, width, target_width);
    for row in span(top, height, target_height) {
        let v = (row as f32 + 0.5 - top) / height;
        for column in columns.clone() {
            let u = (column as f32 + 0.5 - left) / width;
            let sx = origin[0] + u * across[0] + v * down[0];
            let sy = origin[1] + u * across[1] + v * down[1];
            let [b, g, r, a] = bilinear(
                sx * image_width as f32 - 0.5,
                sy * image_height as f32 - 0.5,
                image_width,
                image_height,
                |x, y| {
                    // Premultiplied while blending neighbours, so clear
                    // pixels' colors don't bleed in.
                    let pixel = &bytes[(y * image_width + x) * 4..][..4];
                    let a = f32::from(pixel[3]) / 255.0;
                    let c = |i: usize| f32::from(pixel[i]) / 255.0 * a;
                    [c(0), c(1), c(2), a]
                },
            );
            if a > 0.0 {
                let signal = target.signal([r / a, g / a, b / a]);
                target.blend(column, row, signal, a);
            }
        }
    }
}

/// Draw text subtitles laid out `TEXT_HEIGHT` tall, scaled to the target.
pub fn draw_text(target: &mut impl Target, paints: &[Paint], text_system: &TextSystem) {
    let scale = target.size().1 as f32 / TEXT_HEIGHT;
    let mut glyphs = GlyphCache::default();
    for paint in paints {
        match paint {
            Paint::Box {
                bounds,
                radius,
                color,
            } => fill_rounded(target, *bounds, f32::from(*radius), *color, scale),
            Paint::Glyph(glyph) => {
                if let Some(raster) = glyphs.get(glyph, scale, text_system) {
                    draw_glyph(target, &raster, glyph);
                }
            }
        }
    }
}

/// A rounded box, given at `1 / scale` of the target's size.
pub(crate) fn fill_rounded(
    target: &mut impl Target,
    bounds: Bounds<Pixels>,
    radius: f32,
    color: Hsla,
    scale: f32,
) {
    let Rgba { r, g, b, a } = color.into();
    let signal = target.signal([r, g, b]);
    let (target_width, target_height) = target.size();
    let (left, top) = (
        f32::from(bounds.left()) * scale,
        f32::from(bounds.top()) * scale,
    );
    let (width, height) = (
        f32::from(bounds.size.width) * scale,
        f32::from(bounds.size.height) * scale,
    );
    let (half_width, half_height) = (width / 2.0, height / 2.0);
    let radius = (radius * scale).min(half_width).min(half_height);
    let (center_x, center_y) = (left + half_width, top + half_height);
    let columns = span(left, width, target_width);
    for y in span(top, height, target_height) {
        for x in columns.clone() {
            // How far the pixel's center is outside the box's edge.
            let qx = (x as f32 + 0.5 - center_x).abs() - (half_width - radius);
            let qy = (y as f32 + 0.5 - center_y).abs() - (half_height - radius);
            let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
            let coverage = (0.5 - outside).clamp(0.0, 1.0);
            target.blend(x, y, signal, a * coverage);
        }
    }
}

fn draw_glyph(target: &mut impl Target, raster: &GlyphRaster, glyph: &LineGlyph) {
    let (width, height) = (raster.size.width.0 as usize, raster.size.height.0 as usize);
    let (target_width, target_height) = target.size();
    let Rgba { r, g, b, a } = glyph.color.into();
    let color = target.signal([r, g, b]);
    let bytes_per_pixel = if glyph.is_emoji { 4 } else { 1 };
    if raster.bytes.len() < width * height * bytes_per_pixel {
        return;
    }
    for row in 0..height {
        let Some(y) = offset(raster.origin.y.0, row, target_height) else {
            continue;
        };
        for column in 0..width {
            let Some(x) = offset(raster.origin.x.0, column, target_width) else {
                continue;
            };
            let i = (row * width + column) * bytes_per_pixel;
            if glyph.is_emoji {
                let [blue, green, red, alpha] =
                    [0, 1, 2, 3].map(|c| f32::from(raster.bytes[i + c]) / 255.0);
                let signal = target.signal([red, green, blue]);
                target.blend(x, y, signal, alpha);
            } else {
                let coverage = f32::from(raster.bytes[i]) / 255.0;
                target.blend(x, y, color, a * coverage);
            }
        }
    }
}

/// The pixels from `start` for `length` that are on a side of `limit`
/// pixels.
fn span(start: f32, length: f32, limit: usize) -> std::ops::Range<usize> {
    let first = start.floor().max(0.0) as usize;
    let end = ((start + length).ceil().max(0.0) as usize).min(limit);
    first.min(end)..end
}

/// `origin + step`, if it is on a side of `limit` pixels.
fn offset(origin: i32, step: usize, limit: usize) -> Option<usize> {
    let at = i64::from(origin) + step as i64;
    usize::try_from(at).ok().filter(|&at| at < limit)
}

/// A picture `width` x `height` read at a point between pixels, `pixel`
/// giving each pixel; the edges extend outwards.
pub(crate) fn bilinear<const N: usize>(
    x: f32,
    y: f32,
    width: usize,
    height: usize,
    pixel: impl Fn(usize, usize) -> [f32; N],
) -> [f32; N] {
    let x = x.clamp(0.0, (width - 1) as f32);
    let y = y.clamp(0.0, (height - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let [a, b, c, d] = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| pixel(x, y));
    std::array::from_fn(|i| {
        let top = a[i] + (b[i] - a[i]) * fx;
        let bottom = c[i] + (d[i] - c[i]) * fx;
        top + (bottom - top) * fy
    })
}

/// A glyph at a size, and where it falls within a pixel, in quarters.
type GlyphKey = (FontId, GlyphId, u32, bool, u8, u8);

/// A glyph's raster, and its offset from the pixel its origin is in.
type Offset = (i32, i32, GlyphRaster);

/// Glyphs rasterized so far: outlines draw each one several times over.
#[derive(Default)]
struct GlyphCache {
    rasters: HashMap<GlyphKey, Option<Offset>>,
}

impl GlyphCache {
    fn get(
        &mut self,
        glyph: &LineGlyph,
        scale: f32,
        text_system: &TextSystem,
    ) -> Option<GlyphRaster> {
        let (x, y) = (
            f32::from(glyph.origin.x) * scale,
            f32::from(glyph.origin.y) * scale,
        );
        let quarter = |v: f32| (v.fract().abs() * 4.0).floor() as u8;
        let key = (
            glyph.font_id,
            glyph.glyph_id,
            f32::from(glyph.font_size).to_bits(),
            glyph.is_emoji,
            quarter(x),
            quarter(y),
        );
        let (floor_x, floor_y) = (x.floor() as i32, y.floor() as i32);
        let cached = self.rasters.entry(key).or_insert_with(|| {
            text_system
                .rasterize_line_glyph(glyph, scale)
                .inspect_err(|err| log::debug!("can't rasterize a subtitle glyph: {err:#}"))
                .ok()
                .flatten()
                .map(|raster| {
                    (
                        raster.origin.x.0 - floor_x,
                        raster.origin.y.0 - floor_y,
                        raster,
                    )
                })
        });
        let (dx, dy, raster) = cached.as_ref()?;
        let mut raster = raster.clone();
        raster.origin.x.0 = floor_x + dx;
        raster.origin.y.0 = floor_y + dy;
        Some(raster)
    }
}

#[cfg(test)]
#[path = "tests/overlay.rs"]
mod tests;
