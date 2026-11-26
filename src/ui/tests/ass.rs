use std::rc::Rc;
use std::sync::Arc;

use gpui::TestAppContext;

use super::*;
use crate::media::Orientation;
use crate::media::ass::{self as script, Dialogue, Script};
use crate::media::subtitles::CueContent;
use crate::settings::SubtitleStyle;
use crate::ui::framing::{Adjustments, Framing};
use crate::ui::overlay;
use crate::ui::screenshot::Canvas;
use crate::ui::screenshot::tests::{GREY, canvas, extent};
use crate::ui::subtitles::text_marks;

/// A font every test machine has.
const FONT: &str = "Helvetica";

/// `lines` (each a style's name and text) of a script 640 × 360 whose
/// Default style is in `font`, 36 tall, white with no outline or shadow,
/// its margins 20, 30 and 40; its Box style the same, boxed 6 wide; its
/// Wide style the same, its side margins more than the picture.
fn texts(font: &str, lines: &[(&str, &str)]) -> Vec<Text> {
    let script = Script::parse(&format!(
        "[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, \
         BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV\n\
         Style: Default,{font},36,&H00FFFFFF,&H00000000,&H00000000,1,0,0,2,20,30,40\n\
         Style: Box,{font},36,&H00FFFFFF,&H00000000,&H00000000,3,6,0,2,20,30,40\n\
         Style: Wide,{font},36,&H00FFFFFF,&H00000000,&H00000000,1,0,0,2,400,400,40\n"
    ));
    lines
        .iter()
        .map(|(style, text)| {
            let source = format!("0,0,{style},,0,0,0,,{text}");
            let line = Dialogue::parse(&source);
            script::parse(line.text, Some((&script, &line))).unwrap()
        })
        .collect()
}

/// A picture `width` × `height` turned `turns` quarter turns, shown at
/// its size at the top left of the block, which it fills.
fn place(width: u32, height: u32, turns: i32) -> TextPlace {
    let framing = Framing::new(
        (width, height),
        Orientation::new(turns, false),
        &Adjustments::default(),
    )
    .unwrap();
    let (shown_width, shown_height) = framing.size;
    let picture = Bounds {
        origin: point(px(0.), px(0.)),
        size: size(px(shown_width), px(shown_height)),
    };
    TextPlace {
        picture,
        framing: Some(framing),
        visible: picture,
        area: picture,
        controls_top: None,
    }
}

/// The rows each line of the `texts` wraps into in `place`, where they
/// go and how they line up there.
fn lines_in(
    texts: Vec<Text>,
    style: &SubtitleStyle,
    place: &TextPlace,
) -> Vec<(Rc<WrappedLine>, Bounds<Pixels>, TextAlign)> {
    let mut cx = TestAppContext::with_native_text();
    let cue = Arc::new(CueContent {
        texts,
        bitmaps: Vec::new(),
    });
    let family = SharedString::from(FONT);
    let marks = cx
        .add_empty_window()
        .update(|window, _| text_marks(&[cue], style, &family, place, window));
    cx.quit();
    marks
        .into_iter()
        .filter_map(|mark| match mark {
            TextMark::Line {
                line, area, align, ..
            } => Some((line, area, align)),
            TextMark::Background { .. } => None,
        })
        .collect()
}

/// Where each line of the `texts` goes in `place`, and how its rows line
/// up there.
fn laid_out(
    texts: Vec<Text>,
    style: &SubtitleStyle,
    place: &TextPlace,
) -> Vec<(Bounds<Pixels>, TextAlign)> {
    lines_in(texts, style, place)
        .into_iter()
        .map(|(_, area, align)| (area, align))
        .collect()
}

/// `texts` laid out and drawn as a screenshot draws them, over a grey
/// picture 640 × 360.
fn drawn(texts: Vec<Text>, style: &SubtitleStyle) -> Canvas {
    let mut cx = TestAppContext::with_native_text();
    let text_system = cx.update(|cx| cx.text_system().clone());
    let cue = Arc::new(CueContent {
        texts,
        bitmaps: Vec::new(),
    });
    let framing = Framing::new((640, 360), Orientation::UPRIGHT, &Adjustments::default());
    let font = SharedString::from(FONT);
    let paints = cx.add_empty_window().update(|window, _| {
        overlay::text_overlay(&[cue], style, &font, (640.0, 360.0), framing, window)
    });
    let mut picture = canvas(640, 360, |_, _| [GREY; 3]);
    overlay::draw_text(&mut picture, &paints, &text_system);
    cx.quit();
    picture
}

fn ink(canvas: &Canvas) -> Option<[f32; 4]> {
    extent(canvas, |pixel| pixel.iter().all(|c| *c > 54000))
}

fn shade(canvas: &Canvas) -> Option<[f32; 4]> {
    extent(canvas, |pixel| pixel.iter().all(|c| *c < 11000))
}

#[track_caller]
fn near(seen: f32, expected: f32, slack: f32) {
    assert!(
        (seen - expected).abs() <= slack,
        "{seen}, not {expected} (±{slack})"
    );
}

#[test]
fn lines_go_where_their_alignments_and_margins_say() {
    let lines = [
        ("Default", r"{\an7}Top left"),
        ("Default", r"{\an3}Bottom right"),
        ("Default", r"{\an5}Middle"),
    ];
    let areas = laid_out(
        texts(FONT, &lines),
        &SubtitleStyle::default(),
        &place(640, 360, 0),
    );
    let [(top_left, left), (bottom_right, right), (middle, center)] = areas[..] else {
        panic!("{} lines", areas.len());
    };
    // Wrapped within the margins, a line 36 tall.
    for area in [top_left, bottom_right, middle] {
        near(f32::from(area.size.width), 640.0 - 20.0 - 30.0, 0.01);
        near(f32::from(area.size.height), 36.0, 0.01);
        near(f32::from(area.left()), 20.0, 0.01);
    }
    assert_eq!(
        (left, right, center),
        (TextAlign::Left, TextAlign::Right, TextAlign::Center)
    );
    near(f32::from(top_left.top()), 40.0, 0.01);
    near(f32::from(bottom_right.bottom()), 360.0 - 40.0, 0.01);
    near(f32::from(middle.top()) + 18.0, 180.0, 0.01);

    // At twice the size, and as big again as the settings say.
    let style = SubtitleStyle {
        size: 2.0,
        ..SubtitleStyle::default()
    };
    let areas = laid_out(texts(FONT, &lines), &style, &place(1280, 720, 0));
    near(f32::from(areas[0].0.top()), 80.0, 0.01);
    near(f32::from(areas[0].0.size.height), 36.0 * 2.0 * 2.0, 0.01);
    near(f32::from(areas[1].0.size.width), 1280.0 - 100.0, 0.01);

    // Margins with no room between them leave a quarter of the picture,
    // in its middle.
    let areas = laid_out(
        texts(FONT, &[("Wide", "Squeezed")]),
        &SubtitleStyle::default(),
        &place(640, 360, 0),
    );
    near(f32::from(areas[0].0.size.width), 160.0, 0.01);
    near(f32::from(areas[0].0.left()), 240.0, 0.01);
}

#[test]
fn a_scripts_font_size_is_its_lines_height() {
    // Arial's ascent and descent come to more than its em.
    let lines = lines_in(
        texts("Arial", &[("Default", "Hello")]),
        &SubtitleStyle::default(),
        &place(640, 360, 0),
    );
    let layout = &lines[0].0.unwrapped_layout;
    let (ascent, descent) = (f32::from(layout.ascent), f32::from(layout.descent));
    near(ascent + descent, 36.0, 0.5);
    let em = f32::from(layout.font_size);
    assert!(em < 34.0, "an em of {em}px");
}

#[test]
fn lines_along_an_edge_stack_the_first_nearest_it() {
    let lines = [
        ("Default", "First"),
        ("Default", r"{\an8}Top"),
        ("Default", "Second"),
        ("Default", r"{\an8}Under it"),
    ];
    let style = SubtitleStyle::default();
    let areas = laid_out(texts(FONT, &lines), &style, &place(640, 360, 0));
    let bottoms: Vec<f32> = [0, 2].map(|at| f32::from(areas[at].0.bottom())).into();
    assert_eq!(bottoms, [320.0, 284.0]);
    let tops: Vec<f32> = [1, 3].map(|at| f32::from(areas[at].0.top())).into();
    assert_eq!(tops, [40.0, 76.0]);

    // Above the controls while they show.
    let mut lifted = place(640, 360, 0);
    lifted.controls_top = Some(px(300.));
    let areas = laid_out(texts(FONT, &lines), &style, &lifted);
    let bottom = f32::from(areas[0].0.bottom());
    assert!((290.0..300.0).contains(&bottom), "at {bottom}");
    near(f32::from(areas[2].0.bottom()), bottom - 36.0, 0.01);
    near(f32::from(areas[1].0.top()), 40.0, 0.01);

    // Unless the file's styles are off: then they're as the settings say.
    let plain = SubtitleStyle {
        file_styles: false,
        ..style
    };
    let areas = laid_out(texts(FONT, &lines), &plain, &place(640, 360, 0));
    assert!(areas.iter().all(|(_, align)| *align == TextAlign::Center));
    assert!(areas.iter().all(|(area, _)| f32::from(area.left()) != 20.0));
}

#[test]
fn pos_puts_a_line_where_it_says_on_the_picture() {
    let lines = [
        ("Default", r"{\an5\pos(100,50)}X"),
        ("Default", r"{\an1\pos(600,300)}Y"),
    ];
    let style = SubtitleStyle::default();
    let areas = laid_out(texts(FONT, &lines), &style, &place(640, 360, 0));
    let center = |area: Bounds<Pixels>| {
        let center = area.center();
        (f32::from(center.x), f32::from(center.y))
    };
    let (x, y) = center(areas[0].0);
    near(x, 100.0, 0.01);
    near(y, 50.0, 0.01);
    // Its alignment's corner is at the point: bottom left here.
    assert_eq!(areas[1].1, TextAlign::Left);
    near(f32::from(areas[1].0.left()), 600.0, 0.01);
    near(f32::from(areas[1].0.bottom()), 300.0, 0.01);

    // Turned with the picture, which shows 360 × 640.
    let turned = place(640, 360, 1);
    let areas = laid_out(texts(FONT, &lines), &style, &turned);
    let framing = turned.framing.unwrap();
    let ([x, y, ..], _) = framing.place([100.0 / 640.0, 50.0 / 360.0, 0.0, 0.0]);
    let (seen_x, seen_y) = center(areas[0].0);
    near(seen_x, x * 360.0, 0.01);
    near(seen_y, y * 640.0, 0.01);
    assert!(seen_x > 180.0, "the top left is now at the top right");
    near(f32::from(areas[0].0.size.height), 36.0, 0.01);
}

#[test]
fn a_line_is_drawn_in_its_colors() {
    let style = SubtitleStyle::default();
    let red = drawn(texts(FONT, &[("Default", r"{\c&H0000FF&}HHHH")]), &style);
    let reds = extent(&red, |[r, g, b]| r > 54000 && g < 11000 && b < 11000);
    let [left, top, right, bottom] = reds.expect("red text");
    assert_eq!(ink(&red), None, "and no white");
    assert_eq!(shade(&red), None, "no outline or shadow");
    // Centered between its margins, 36 tall with capitals about 0.7 of it.
    near((left + right) / 2.0, (20.0 + 610.0) / 2.0, 3.0);
    assert!(bottom <= 320.0 && bottom > 300.0, "rows {top}..{bottom}");
    let capitals = bottom - top + 1.0;
    assert!((18.0..29.0).contains(&capitals), "{capitals}px capitals");

    let faint = drawn(texts(FONT, &[("Default", r"{\1a&HFF&}HHHH")]), &style);
    assert_eq!(ink(&faint), None, "all see-through");
}

#[test]
fn outlines_shadows_and_boxes_are_as_wide_as_the_script_says() {
    let style = SubtitleStyle::default();
    let draw = |style_name: &str, text: &str| drawn(texts(FONT, &[(style_name, text)]), &style);
    let plain = draw("Default", "HHHH");
    let [left, top, right, bottom] = ink(&plain).unwrap();

    let outlined = draw("Default", r"{\bord4}HHHH");
    let [outline_left, outline_top, outline_right, outline_bottom] = shade(&outlined).unwrap();
    near(outline_left, left - 4.0, 1.5);
    near(outline_right, right + 4.0, 1.5);
    near(outline_top, top - 4.0, 1.5);
    near(outline_bottom, bottom + 4.0, 1.5);

    let shadowed = draw("Default", r"{\shad4\4a&H00&}HHHH");
    let [_, _, shadow_right, shadow_bottom] = shade(&shadowed).unwrap();
    near(shadow_right, right + 4.0, 1.5);
    near(shadow_bottom, bottom + 4.0, 1.5);
    // An opaque shadow is the outlined text's; a see-through one is the
    // text's alone, drawn once.
    let row = ((top + bottom) / 2.0) as usize;
    let past_outline = |canvas: &Canvas| canvas.pixels[row * canvas.width + right as usize + 6];
    let opaque = draw("Default", r"{\bord3\shad4\4a&H00&}HHHH");
    assert!(
        past_outline(&opaque)[0] < 11000,
        "{:?}",
        past_outline(&opaque)
    );
    let see_through = draw("Default", r"{\bord3\shad4\4a&H80&}HHHH");
    assert_eq!(past_outline(&see_through), [GREY; 3]);
    let see_through = draw("Default", r"{\shad4\4a&H80&}HHHH");
    assert_eq!(
        shade(&see_through),
        None,
        "no darker where copies would meet"
    );

    let boxed = draw("Box", "HHHH");
    let [box_left, box_top, box_right, box_bottom] = shade(&boxed).unwrap();
    // Out from where the row's glyphs start and end, a line tall, and
    // solid between the letters.
    assert!(
        box_left < left - 6.0 && box_left > left - 12.0,
        "{box_left}"
    );
    assert!(
        box_right > right + 6.0 && box_right < right + 12.0,
        "{box_right}"
    );
    near(box_bottom - box_top + 1.0, 36.0 + 12.0, 1.5);
    let solid =
        (left as usize..right as usize).all(|x| boxed.pixels[row * boxed.width + x] != [GREY; 3]);
    assert!(solid, "the box is solid");
    // Lined up with its rows, as they're lined up.
    let [_, _, right_box, _] = shade(&draw("Box", r"{\an3}HHHH")).unwrap();
    near(right_box, 610.0 + 6.0, 1.5);
}

#[test]
fn underlines_and_strikethroughs_rule_across_the_letters() {
    let style = SubtitleStyle::default();
    let draw = |text: &str| drawn(texts(FONT, &[("Default", text)]), &style);
    let plain = draw("HH HH");
    let [left, top, right, bottom] = ink(&plain).unwrap();
    let underlined = draw(r"{\u1}HH HH");
    let [_, _, _, underline_bottom] = ink(&underlined).unwrap();
    // Down in the descent, clear of the letters.
    assert!(
        underline_bottom > bottom + (bottom - top) * 0.1,
        "{underline_bottom} under {bottom}"
    );
    // A row across the letters, the space between them too.
    let across = |canvas: &Canvas| {
        (top as usize..=bottom as usize).any(|y| {
            (left as usize..=right as usize).all(|x| canvas.pixels[y * canvas.width + x][0] > 54000)
        })
    };
    assert!(!across(&plain));
    assert!(across(&draw(r"{\s1}HH HH")));
}

#[test]
fn outlines_ring_their_letters_more_densely_the_wider_they_are() {
    let radii = |width: f32| {
        let mut radii: Vec<(u32, usize)> = Vec::new();
        for (x, y) in ring(width) {
            let radius = (x.hypot(y) * 100.0).round() as u32;
            match radii.last_mut() {
                Some((last, count)) if *last == radius => *count += 1,
                _ => radii.push((radius, 1)),
            }
        }
        radii
    };
    assert!(ring(0.0).is_empty());
    assert_eq!(radii(1.5), [(150, 8)], "a thin one, 8 around");
    assert_eq!(radii(2.5), [(250, 16)], "past 2, 16");
    assert_eq!(radii(6.0), [(300, 16), (600, 16)], "a ring each 3 out");
    assert_eq!(
        radii(30.0),
        [(1000, 16), (2000, 16), (3000, 16)],
        "3 at most"
    );
}

#[test]
fn a_font_not_installed_is_the_one_the_settings_give() {
    // Not the one gpui falls back to, so it shows which was taken.
    let style = SubtitleStyle {
        font: "Georgia".to_owned(),
        ..SubtitleStyle::default()
    };
    let hello = |font: &str| drawn(texts(font, &[("Default", "Hello")]), &style);
    let (settings, other) = (hello("Georgia"), hello(FONT));
    assert!(ink(&settings).is_some());
    assert!(settings.pixels != other.pixels, "the fonts differ");
    for missing in ["No Such Font Anywhere", ""] {
        assert!(hello(missing).pixels == settings.pixels, "{missing:?}");
    }
}
