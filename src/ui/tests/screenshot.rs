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
