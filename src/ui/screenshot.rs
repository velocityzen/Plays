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
mod tests {
    use gpui::{Bounds, Hsla, Rgba, TestAppContext, point, px, size};

    use super::*;
    use crate::media::subtitles::{CueContent, Placement, Span, Text};
    use crate::settings::{SubtitleEdge, SubtitleStyle};
    use crate::ui::theme::pick_ui_font;
    use crate::ui::theme::subtitles::{
        BACKGROUND_PADDING, EDGE_WIDTH, FONT_SIZE, LINE_HEIGHT, WRAP_WIDTH,
    };

    /// The picture subtitles are drawn over in these tests: mid grey, so
    /// the white text is lighter and its black outline or box darker.
    const GREY: u16 = 32768;

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
        overlay::draw_image(
            &mut picture,
            &image,
            [0.5, 0.5, 0.5, 0.5],
            ContentTransform::IDENTITY,
        );
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
        overlay::fill_rounded(&mut picture, bounds, 0.0, red, 2.0);
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

    /// Text subtitles laid out and drawn as a screenshot draws them, with
    /// the system's own fonts, over a grey picture `width` × `height`.
    fn subtitled(width: usize, height: usize, text: &str, style: &SubtitleStyle) -> Canvas {
        let mut cx = TestAppContext::with_native_text();
        let (font, text_system) = cx.update(|cx| (pick_ui_font(cx), cx.text_system().clone()));
        let cue = Arc::new(CueContent {
            texts: vec![Text {
                text: text.into(),
                spans: vec![Span {
                    len: text.len(),
                    ..Span::default()
                }],
                placement: Placement::Bottom,
            }],
            bitmaps: Vec::new(),
        });
        let paints = cx.add_empty_window().update(|window, _| {
            overlay::text_overlay(&[cue], style, &font, (width as f32, height as f32), window)
        });
        let mut picture = canvas(width, height, |_, _| [GREY; 3]);
        overlay::draw_text(&mut picture, &paints, &text_system);
        cx.quit();
        picture
    }

    /// The pixels `pick` picks, as their left, top, right and bottom
    /// (inclusive), if there are any.
    fn extent(canvas: &Canvas, pick: impl Fn([u16; 3]) -> bool) -> Option<[f32; 4]> {
        let mut found: Option<[usize; 4]> = None;
        for (i, pixel) in canvas.pixels.iter().enumerate() {
            if pick(*pixel) {
                let (x, y) = (i % canvas.width, i / canvas.width);
                let [left, top, right, bottom] = found.get_or_insert([x, y, x, y]);
                *left = (*left).min(x);
                *top = (*top).min(y);
                *right = (*right).max(x);
                *bottom = (*bottom).max(y);
            }
        }
        found.map(|bounds| bounds.map(|edge| edge as f32))
    }

    /// The white text, at least two thirds in.
    fn ink(canvas: &Canvas) -> Option<[f32; 4]> {
        extent(canvas, |pixel| pixel[0] > 54000)
    }

    /// Black outline or box, at least two thirds over the grey.
    fn shade(canvas: &Canvas) -> Option<[f32; 4]> {
        extent(canvas, |pixel| pixel[0] < 11000)
    }

    fn plain() -> SubtitleStyle {
        SubtitleStyle {
            edge: SubtitleEdge::None,
            ..SubtitleStyle::default()
        }
    }

    #[test]
    fn draws_text_subtitles_at_the_size_and_height_settings_give() {
        let (width, height) = (640.0, 360.0);
        let style = plain();
        let shot = subtitled(640, 360, "HHHH", &style);
        let [left, top, right, bottom] = ink(&shot).expect("the cue is drawn");
        assert!(
            ((left + right) / 2.0 - (width - 1.0) / 2.0).abs() <= 2.0,
            "centered: {left}..{right}"
        );
        // Capitals are about 0.7 em, and the font 5% of the picture.
        let font = height * FONT_SIZE;
        let capitals = bottom - top + 1.0;
        assert!(
            (0.6 * font..=0.8 * font).contains(&capitals),
            "{capitals}px capitals in an {font}px font"
        );
        // In its line, which sits `position` of the picture above the bottom.
        let line_bottom = height * (1.0 - style.position);
        let line_top = line_bottom - font * LINE_HEIGHT;
        assert!(
            top >= line_top - 1.0 && bottom <= line_bottom + 1.0,
            "rows {top}..{bottom} in a line at {line_top}..{line_bottom}"
        );
        assert_eq!(shade(&shot), None, "no outline or box");

        let big = SubtitleStyle {
            size: 2.0,
            ..plain()
        };
        let [_, big_top, _, big_bottom] = ink(&subtitled(640, 360, "HHHH", &big)).unwrap();
        let grown = (big_bottom - big_top + 1.0) / capitals;
        assert!((1.8..=2.2).contains(&grown), "twice the size: {grown}×");

        let high = SubtitleStyle {
            position: 0.4,
            ..plain()
        };
        let [_, _, _, high_bottom] = ink(&subtitled(640, 360, "HHHH", &high)).unwrap();
        let raised = bottom - high_bottom;
        let expected = (high.position - style.position) * height;
        assert!(
            (raised - expected).abs() <= 1.0,
            "raised {raised}px, not {expected}px"
        );
    }

    #[test]
    fn text_subtitles_scale_with_the_picture() {
        let small = ink(&subtitled(640, 360, "HHHH", &plain())).unwrap();
        let large = ink(&subtitled(1280, 720, "HHHH", &plain())).unwrap();
        let size = |[left, top, right, bottom]: [f32; 4]| (right - left + 1.0, bottom - top + 1.0);
        let ((small_width, small_height), (large_width, large_height)) = (size(small), size(large));
        // Twice as big, give or take the pixels edges round to.
        for (small, large, what) in [
            (small_width, large_width, "wide"),
            (small_height, large_height, "tall"),
        ] {
            assert!(
                (large - 2.0 * small).abs() <= 2.0,
                "{large}px {what} at twice the size, {small}px at once"
            );
        }
    }

    #[test]
    fn long_text_subtitles_wrap_inside_the_picture() {
        let (width, height) = (180.0, 320.0);
        let text = "A subtitle far too long to fit on one row of a narrow picture";
        let [left, top, right, bottom] = ink(&subtitled(180, 320, text, &plain())).unwrap();
        let margin = width * (1.0 - WRAP_WIDTH) / 2.0;
        assert!(
            left >= margin - 2.0 && right <= width - margin + 2.0,
            "{left}..{right} inside {margin}..{}",
            width - margin
        );
        let line = height * FONT_SIZE * LINE_HEIGHT;
        assert!(
            bottom - top > 1.5 * line,
            "wrapped into rows: {top}..{bottom}"
        );
    }

    #[test]
    fn draws_the_outline_or_box_settings_give() {
        let font = 360.0 * FONT_SIZE;

        let outlined = subtitled(640, 360, "HHHH", &SubtitleStyle::default());
        let text = ink(&outlined).unwrap();
        let outline = shade(&outlined).expect("an outline");
        let width = (font * EDGE_WIDTH).max(1.0);
        for (beyond, side) in [
            (text[0] - outline[0], "left"),
            (text[1] - outline[1], "top"),
            (outline[2] - text[2], "right"),
            (outline[3] - text[3], "bottom"),
        ] {
            assert!(
                (0.5..=width + 1.5).contains(&beyond),
                "the outline goes {beyond}px beyond the text on the {side}"
            );
        }

        let boxed = SubtitleStyle {
            background_opacity: 1.0,
            ..plain()
        };
        let shot = subtitled(640, 360, "HHHH", &boxed);
        let text = ink(&shot).unwrap();
        let back = shade(&shot).expect("a box");
        // Padded either side, past the capitals' own sides, and a line tall.
        let padding = font * BACKGROUND_PADDING;
        for (beyond, side) in [(text[0] - back[0], "left"), (back[2] - text[2], "right")] {
            assert!(
                (padding - 1.0..=padding + 4.0).contains(&beyond),
                "the box is {beyond}px wider than the text on the {side}"
            );
        }
        let tall = back[3] - back[1] + 1.0;
        assert!(
            (tall - font * LINE_HEIGHT).abs() <= 1.5,
            "a {tall}px box for a {}px line",
            font * LINE_HEIGHT
        );
        assert!(back[1] < text[1] && back[3] > text[3]);
    }
}
