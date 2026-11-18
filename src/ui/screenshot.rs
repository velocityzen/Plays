//! Screenshots: the frame a tile shows, saved as a PNG or JPEG file at the
//! video's full resolution, turned, reshaped and cropped as it shows, with
//! the subtitles showing drawn in.
//!
//! The frame is decoded again on its own (`media::still`), since what the
//! tile has may be scaled down or on the GPU. Bitmap subtitles go onto it
//! where the stream places them, before the picture is turned and cropped,
//! as on screen. Text subtitles are laid out as on a tile showing just the
//! picture, `TEXT_HEIGHT` pixels tall, then drawn at the picture's size
//! with the glyphs gpui draws on screen. HDR video is tone mapped, or kept
//! in a 16-bit PNG whose cICP chunk says it is BT.2020 PQ or HLG; subtitles
//! go over it at HDR reference white.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gpui::{
    Bounds, FontId, GlyphId, GlyphRaster, Hsla, LineGlyph, Pixels, RenderImage, Rgba, SharedString,
    TextAlign, TextSystem, Window, point, px, size,
};

use super::framing::{Adjustments, Framing};
use super::subtitles::{self, TextMark, TextPlace};
use super::theme::subtitles::TEXT_HEIGHT;
use crate::media::hdr::{self, Transfer};
use crate::media::picture::rows;
use crate::media::still::{self, Still};
use crate::media::subtitles::CueContent;
use crate::media::{Orientation, Source};
use crate::settings::{ScreenshotFormat, ScreenshotSettings, SubtitleStyle};

/// What to save, gathered on the UI thread.
pub struct Request {
    pub source: Source,
    /// The video track, by stream index.
    pub stream: usize,
    /// When the frame shows, in the stream's time base.
    pub timestamp: i64,
    /// When it shows, in seconds, for the file's name.
    pub position: f64,
    /// How the file says to turn the picture, and how it was adjusted.
    pub orientation: Orientation,
    pub adjustments: Adjustments,
    /// Bitmap subtitles: BGRA images and where they go on the picture.
    pub bitmaps: Vec<(Arc<RenderImage>, [f32; 4])>,
    /// Text subtitles, laid out over a picture `TEXT_HEIGHT` tall.
    pub text: Vec<Paint>,
    pub text_system: Arc<TextSystem>,
    pub settings: ScreenshotSettings,
    pub folder: PathBuf,
}

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

/// Decode the frame, draw it as it shows and save it. The file written.
pub fn take(request: Request) -> Result<PathBuf> {
    let settings = &request.settings;
    let still = still::grab(
        &request.source,
        request.stream,
        request.timestamp,
        settings.keeps_hdr(),
    )?;
    let mut canvas = Canvas::from(still);
    for (image, rect) in &request.bitmaps {
        canvas.draw_image(image, *rect);
    }
    let framing = Framing::new(
        (canvas.width as u32, canvas.height as u32),
        request.orientation,
        &request.adjustments,
    )
    .context("the picture has no size")?;
    let mut canvas = canvas.framed(&framing);
    canvas.draw_text(&request.text, &request.text_system);

    let bytes = canvas.encode(settings)?;
    let name = file_name(&request.source.name(), request.position);
    save(&request.folder, &name, settings.format.extension(), &bytes)
}

/// The name a screenshot is saved as, without its extension: the file's
/// own and where in it, `movie 01-23-45.678`.
pub fn file_name(media: &str, position: f64) -> String {
    let stem = Path::new(media)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem: String = stem
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim();
    let stem = if stem.is_empty() { "Plays" } else { stem };
    let millis = (position.max(0.0) * 1000.0).round() as u64;
    let (hours, minutes) = (millis / 3_600_000, millis / 60_000 % 60);
    let (seconds, millis) = (millis / 1000 % 60, millis % 1000);
    format!("{stem} {hours:02}-{minutes:02}-{seconds:02}.{millis:03}")
}

/// Write `bytes` to `name.extension` in `folder`, or `name 2.extension`
/// and so on if that is taken, never replacing a file.
fn save(folder: &Path, name: &str, extension: &str, bytes: &[u8]) -> Result<PathBuf> {
    fs::create_dir_all(folder)
        .with_context(|| format!("can't make the folder {}", folder.display()))?;
    for n in 1.. {
        let file = match n {
            1 => format!("{name}.{extension}"),
            n => format!("{name} {n}.{extension}"),
        };
        let path = folder.join(file);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                let written = write_all(file, bytes);
                if written.is_err() {
                    let _ = fs::remove_file(&path);
                }
                written.with_context(|| format!("can't write {}", path.display()))?;
                return Ok(path);
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("can't write to {}", folder.display()));
            }
        }
    }
    unreachable!("some name is free")
}

fn write_all(file: File, bytes: &[u8]) -> io::Result<()> {
    let mut writer = BufWriter::new(file);
    writer.write_all(bytes)?;
    writer.flush()
}

/// A picture being drawn on: RGB, 16 bits a sample, in sRGB or an HDR
/// signal.
struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<[u16; 3]>,
    hdr: Option<Transfer>,
}

impl From<Still> for Canvas {
    fn from(still: Still) -> Self {
        Self {
            width: still.width as usize,
            height: still.height as usize,
            pixels: still.pixels,
            hdr: still.hdr,
        }
    }
}

impl Canvas {
    /// An sRGB color in this canvas's signal.
    fn signal(&self, srgb: [f32; 3]) -> [f32; 3] {
        match self.hdr {
            None => srgb,
            Some(transfer) => hdr::sdr_to_hdr(srgb, transfer),
        }
    }

    /// Lay `signal` over the pixel at `x`, `y` with `alpha`.
    fn blend(&mut self, x: usize, y: usize, signal: [f32; 3], alpha: f32) {
        if alpha <= 0.0 {
            return;
        }
        let alpha = alpha.min(1.0);
        let pixel = &mut self.pixels[y * self.width + x];
        for (sample, signal) in pixel.iter_mut().zip(signal) {
            let base = f32::from(*sample) / 65535.0;
            let mixed = signal * alpha + base * (1.0 - alpha);
            *sample = (mixed.clamp(0.0, 1.0) * 65535.0).round() as u16;
        }
    }

    /// Draw a BGRA image (straight alpha) stretched over `rect`, in
    /// fractions of the canvas.
    fn draw_image(&mut self, image: &RenderImage, rect: [f32; 4]) {
        let (Some(bytes), image_size) = (image.as_bytes(0), image.size(0)) else {
            return;
        };
        let (image_width, image_height) =
            (image_size.width.0 as usize, image_size.height.0 as usize);
        if image_width == 0 || image_height == 0 || bytes.len() < image_width * image_height * 4 {
            return;
        }
        let [x, y, width, height] = rect;
        let (left, top) = (x * self.width as f32, y * self.height as f32);
        let (width, height) = (width * self.width as f32, height * self.height as f32);
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let columns = span(left, width, self.width);
        for row in span(top, height, self.height) {
            let v = (row as f32 + 0.5 - top) / height * image_height as f32 - 0.5;
            for column in columns.clone() {
                let u = (column as f32 + 0.5 - left) / width * image_width as f32 - 0.5;
                let [b, g, r, a] = bilinear(u, v, image_width, image_height, |x, y| {
                    // Premultiplied while blending neighbours, so clear
                    // pixels' colors don't bleed in.
                    let pixel = &bytes[(y * image_width + x) * 4..][..4];
                    let a = f32::from(pixel[3]) / 255.0;
                    let c = |i: usize| f32::from(pixel[i]) / 255.0 * a;
                    [c(0), c(1), c(2), a]
                });
                if a > 0.0 {
                    let signal = self.signal([r / a, g / a, b / a]);
                    self.blend(column, row, signal, a);
                }
            }
        }
    }

    /// The picture as it shows: turned, reshaped and cropped.
    fn framed(self, framing: &Framing) -> Self {
        let content = framing.content();
        let width = (framing.size.0.round() as usize).max(1);
        let height = (framing.size.1.round() as usize).max(1);
        let untouched = [content.origin, content.across, content.down]
            .concat()
            .iter()
            .zip([0.0, 0.0, 1.0, 0.0, 0.0, 1.0])
            .all(|(a, b)| (a - b).abs() < 1e-6);
        if untouched && (width, height) == (self.width, self.height) {
            return self;
        }
        let (source_width, source_height) = (self.width, self.height);
        let source = &self.pixels;
        let pixels = rows(width, height, |y, out| {
            let v = (y as f32 + 0.5) / height as f32;
            for (x, out) in out.iter_mut().enumerate() {
                let u = (x as f32 + 0.5) / width as f32;
                let sx = content.origin[0] + u * content.across[0] + v * content.down[0];
                let sy = content.origin[1] + u * content.across[1] + v * content.down[1];
                let sample = bilinear(
                    sx * source_width as f32 - 0.5,
                    sy * source_height as f32 - 0.5,
                    source_width,
                    source_height,
                    |x, y| source[y * source_width + x].map(f32::from),
                );
                *out = sample.map(|c| c.round() as u16);
            }
        });
        Self {
            width,
            height,
            pixels,
            hdr: self.hdr,
        }
    }

    /// Draw text subtitles laid out `TEXT_HEIGHT` tall, scaled to the canvas.
    fn draw_text(&mut self, paints: &[Paint], text_system: &TextSystem) {
        let scale = self.height as f32 / TEXT_HEIGHT;
        let mut glyphs = GlyphCache::default();
        for paint in paints {
            match paint {
                Paint::Box {
                    bounds,
                    radius,
                    color,
                } => self.fill_rounded(*bounds, f32::from(*radius), *color, scale),
                Paint::Glyph(glyph) => {
                    if let Some(raster) = glyphs.get(glyph, scale, text_system) {
                        self.draw_glyph(&raster, glyph);
                    }
                }
            }
        }
    }

    /// A rounded box, given at `1 / scale` of the canvas's size.
    fn fill_rounded(&mut self, bounds: Bounds<Pixels>, radius: f32, color: Hsla, scale: f32) {
        let Rgba { r, g, b, a } = color.into();
        let signal = self.signal([r, g, b]);
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
        let columns = span(left, width, self.width);
        for y in span(top, height, self.height) {
            for x in columns.clone() {
                // How far the pixel's center is outside the box's edge.
                let qx = (x as f32 + 0.5 - center_x).abs() - (half_width - radius);
                let qy = (y as f32 + 0.5 - center_y).abs() - (half_height - radius);
                let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                let coverage = (0.5 - outside).clamp(0.0, 1.0);
                self.blend(x, y, signal, a * coverage);
            }
        }
    }

    fn draw_glyph(&mut self, raster: &GlyphRaster, glyph: &LineGlyph) {
        let (width, height) = (raster.size.width.0 as usize, raster.size.height.0 as usize);
        let Rgba { r, g, b, a } = glyph.color.into();
        let color = self.signal([r, g, b]);
        let bytes_per_pixel = if glyph.is_emoji { 4 } else { 1 };
        if raster.bytes.len() < width * height * bytes_per_pixel {
            return;
        }
        for row in 0..height {
            let Some(y) = offset(raster.origin.y.0, row, self.height) else {
                continue;
            };
            for column in 0..width {
                let Some(x) = offset(raster.origin.x.0, column, self.width) else {
                    continue;
                };
                let i = (row * width + column) * bytes_per_pixel;
                if glyph.is_emoji {
                    let [blue, green, red, alpha] =
                        [0, 1, 2, 3].map(|c| f32::from(raster.bytes[i + c]) / 255.0);
                    let signal = self.signal([red, green, blue]);
                    self.blend(x, y, signal, alpha);
                } else {
                    let coverage = f32::from(raster.bytes[i]) / 255.0;
                    self.blend(x, y, color, a * coverage);
                }
            }
        }
    }

    /// The canvas as a file in the format asked for.
    fn encode(&self, settings: &ScreenshotSettings) -> Result<Vec<u8>> {
        let (width, height) = (self.width as u32, self.height as u32);
        let mut bytes = Vec::new();
        match (settings.format, self.hdr) {
            (ScreenshotFormat::Png, Some(transfer)) => {
                let mut encoder = png::Encoder::new(&mut bytes, width, height);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Sixteen);
                let mut writer = encoder.write_header()?;
                // BT.2020 primaries, PQ (16) or HLG (18), RGB, full range.
                let transfer = match transfer {
                    Transfer::Pq => 16,
                    Transfer::Hlg => 18,
                };
                writer.write_chunk(png::chunk::cICP, &[9, transfer, 0, 1])?;
                let samples: Vec<u8> = self
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.iter().flat_map(|sample| sample.to_be_bytes()))
                    .collect();
                writer.write_image_data(&samples)?;
                writer.finish()?;
            }
            (ScreenshotFormat::Png, None) => {
                let mut encoder = png::Encoder::new(&mut bytes, width, height);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
                let mut writer = encoder.write_header()?;
                writer.write_image_data(&self.rgb8())?;
                writer.finish()?;
            }
            (ScreenshotFormat::Jpeg, _) => {
                image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut bytes,
                    settings.jpeg_quality(),
                )
                .encode(
                    &self.rgb8(),
                    width,
                    height,
                    image::ExtendedColorType::Rgb8,
                )?;
            }
        }
        Ok(bytes)
    }

    fn rgb8(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|pixel| pixel.map(|sample| ((u32::from(sample) * 255 + 32767) / 65535) as u8))
            .collect()
    }
}

/// The pixels from `start` for `length` that are on a canvas side of
/// `limit` pixels.
fn span(start: f32, length: f32, limit: usize) -> std::ops::Range<usize> {
    let first = start.floor().max(0.0) as usize;
    let end = ((start + length).ceil().max(0.0) as usize).min(limit);
    first.min(end)..end
}

/// `origin + step`, if it is on a canvas side of `limit` pixels.
fn offset(origin: i32, step: usize, limit: usize) -> Option<usize> {
    let at = i64::from(origin) + step as i64;
    usize::try_from(at).ok().filter(|&at| at < limit)
}

/// A picture `width` x `height` read at a point between pixels, `pixel`
/// giving each pixel; the edges extend outwards.
fn bilinear<const N: usize>(
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
mod tests {
    use super::*;

    fn canvas(width: usize, height: usize, pixel: impl Fn(usize, usize) -> [u16; 3]) -> Canvas {
        Canvas {
            width,
            height,
            pixels: (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| pixel(x, y))
                .collect(),
            hdr: None,
        }
    }

    #[test]
    fn names_files_by_media_and_position() {
        assert_eq!(file_name("movie.mkv", 5025.678), "movie 01-23-45.678");
        assert_eq!(file_name("a.b.mp4", 0.0), "a.b 00-00-00.000");
        assert_eq!(file_name("clip", 59.9996), "clip 00-01-00.000");
        assert_eq!(file_name("live?id=3", 1.5), "live-id=3 00-00-01.500");
        assert_eq!(file_name("", 1.0), "Plays 00-00-01.000");
    }

    #[test]
    fn saves_without_replacing() {
        let folder = std::env::temp_dir().join(format!("plays-shots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let first = save(&folder, "clip 00-00-01.000", "png", b"one").unwrap();
        let second = save(&folder, "clip 00-00-01.000", "png", b"two").unwrap();
        assert_eq!(first.file_name().unwrap(), "clip 00-00-01.000.png");
        assert_eq!(second.file_name().unwrap(), "clip 00-00-01.000 2.png");
        assert_eq!(fs::read(&first).unwrap(), b"one");
        assert_eq!(fs::read(&second).unwrap(), b"two");
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn frames_the_picture_as_it_shows() {
        // A 4 x 2 picture whose pixels say where they are.
        let picture = || canvas(4, 2, |x, y| [x as u16, y as u16, 0]);
        let at = |canvas: &Canvas, x: usize, y: usize| canvas.pixels[y * canvas.width + x];
        let framing = |turns, mirrored| {
            let adjustments = Adjustments {
                orientation: Orientation::new(turns, mirrored),
                ..Adjustments::default()
            };
            Framing::new((4, 2), Orientation::UPRIGHT, &adjustments).unwrap()
        };
        let plain = picture().framed(&framing(0, false));
        assert_eq!(plain.pixels, picture().pixels);
        // A quarter turn clockwise: the top left shows the bottom left.
        let turned = picture().framed(&framing(1, false));
        assert_eq!((turned.width, turned.height), (2, 4));
        assert_eq!(at(&turned, 0, 0), [0, 1, 0]);
        assert_eq!(at(&turned, 1, 0), [0, 0, 0]);
        assert_eq!(at(&turned, 0, 3), [3, 1, 0]);
        let mirrored = picture().framed(&framing(0, true));
        assert_eq!(at(&mirrored, 0, 0), [3, 0, 0]);
        assert_eq!(at(&mirrored, 3, 1), [0, 1, 0]);
        // Cropped square: the middle two columns.
        let square = Framing::new(
            (4, 2),
            Orientation::UPRIGHT,
            &Adjustments {
                crop: Some(crate::ui::framing::CROPS[0]),
                ..Adjustments::default()
            },
        )
        .unwrap();
        let cropped = picture().framed(&square);
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(at(&cropped, 0, 0), [1, 0, 0]);
        assert_eq!(at(&cropped, 1, 1), [2, 1, 0]);
    }

    #[test]
    fn draws_bitmaps_and_boxes_over_the_picture() {
        let mut picture = canvas(8, 8, |_, _| [0; 3]);
        // A 2 x 2 image: opaque white on the left, clear on the right.
        let pixels = [
            255, 255, 255, 255, 0, 0, 0, 0, //
            255, 255, 255, 255, 0, 0, 0, 0,
        ];
        let image = RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::from_raw(2, 2, pixels.to_vec()).unwrap(),
        )]);
        picture.draw_image(&image, [0.5, 0.5, 0.5, 0.5]);
        let at = |canvas: &Canvas, x: usize, y: usize| canvas.pixels[y * canvas.width + x];
        assert_eq!(at(&picture, 4, 4), [65535; 3]);
        assert_eq!(at(&picture, 7, 7), [0; 3]);
        assert_eq!(at(&picture, 3, 3), [0; 3]);

        let mut picture = canvas(8, 8, |_, _| [0; 3]);
        let bounds = Bounds {
            origin: point(px(1.), px(1.)),
            size: size(px(2.), px(2.)),
        };
        let red = Hsla::from(Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 0.5,
        });
        picture.fill_rounded(bounds, 0.0, red, 2.0);
        assert_eq!(at(&picture, 2, 2), [32768, 0, 0]);
        assert_eq!(at(&picture, 5, 5), [32768, 0, 0]);
        assert_eq!(at(&picture, 1, 1), [0; 3]);
        assert_eq!(at(&picture, 6, 6), [0; 3]);
    }

    #[test]
    fn subtitles_over_hdr_are_at_reference_white() {
        let mut picture = canvas(1, 1, |_, _| [0; 3]);
        picture.hdr = Some(Transfer::Pq);
        let white = picture.signal([1.0; 3]);
        picture.blend(0, 0, white, 1.0);
        let reference = (0.5806 * 65535.0) as i32;
        assert!(
            picture.pixels[0]
                .iter()
                .all(|&s| (i32::from(s) - reference).abs() < 100)
        );
    }

    #[test]
    fn encodes_png_and_jpeg() {
        let picture = canvas(16, 8, |x, y| [x as u16 * 4000, y as u16 * 8000, 30000]);
        let read = |bytes: Vec<u8>| {
            let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
                .read_info()
                .unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            reader.next_frame(&mut pixels).unwrap();
            (reader.info().clone(), pixels)
        };
        let (info, pixels) = read(picture.encode(&ScreenshotSettings::default()).unwrap());
        assert_eq!((info.width, info.height), (16, 8));
        assert_eq!(
            (info.color_type, info.bit_depth),
            (png::ColorType::Rgb, png::BitDepth::Eight)
        );
        assert!(info.srgb.is_some() && info.coding_independent_code_points.is_none());
        // The last pixel of the first row: x 15, y 0.
        let last = 15 * 3;
        assert_eq!(&pixels[last..last + 3], &[233, 0, 117]);

        let mut hdr = canvas(16, 8, |_, _| [40000; 3]);
        hdr.hdr = Some(Transfer::Hlg);
        let (info, pixels) = read(hdr.encode(&ScreenshotSettings::default()).unwrap());
        assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
        let cicp = info.coding_independent_code_points.expect("a cICP chunk");
        assert_eq!(
            [
                cicp.color_primaries,
                cicp.transfer_function,
                cicp.matrix_coefficients
            ],
            [9, 18, 0]
        );
        assert!(cicp.is_video_full_range_image);
        assert_eq!(u16::from_be_bytes([pixels[0], pixels[1]]), 40000);

        let jpeg_settings = ScreenshotSettings {
            format: ScreenshotFormat::Jpeg,
            ..ScreenshotSettings::default()
        };
        let jpeg = picture.encode(&jpeg_settings).unwrap();
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (16, 8));
    }
}
