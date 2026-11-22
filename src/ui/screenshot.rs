//! Screenshots: the frame a tile shows, saved as a PNG or JPEG file at the
//! video's full resolution, turned, reshaped and cropped as it shows, with
//! the subtitles showing drawn in.
//!
//! The frame is decoded again on its own (`media::still`), since what the
//! tile has may be scaled down or on the GPU. Bitmap subtitles go onto it
//! where the stream places them, before the picture is turned and cropped,
//! as on screen; text subtitles after, as `ui::overlay` draws them. HDR video is tone mapped, or kept
//! in a 16-bit PNG whose cICP chunk says it is BT.2020 PQ or HLG; subtitles
//! go over it at HDR reference white.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gpui::{ContentTransform, RenderImage, TextSystem};

use super::framing::{Adjustments, Framing};
use super::overlay::{self, Paint, Target, bilinear};
use crate::media::hdr::{self, Transfer};
use crate::media::picture::rows;
use crate::media::still::{self, Still};
use crate::media::{Orientation, Source};
use crate::settings::{ScreenshotFormat, ScreenshotSettings};

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
        overlay::draw_image(&mut canvas, image, *rect, ContentTransform::IDENTITY);
    }
    let framing = Framing::new(
        (canvas.width as u32, canvas.height as u32),
        request.orientation,
        &request.adjustments,
    )
    .context("the picture has no size")?;
    let mut canvas = canvas.framed(&framing);
    overlay::draw_text(&mut canvas, &request.text, &request.text_system);

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

impl Target for Canvas {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

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
}

#[cfg(test)]
#[path = "tests/screenshot.rs"]
mod tests;
