use super::*;
use crate::media::subtitles::Span;

const FORMAT_LINE: &str = "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, \
                           OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, \
                           ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, \
                           MarginL, MarginR, MarginV, Encoding";

/// A script 640 × 360 with a Default style and a boxed, bold Sign style.
fn script() -> Script {
    Script::parse(&format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\n\n\
         [V4+ Styles]\n{FORMAT_LINE}\n\
         Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,1.5,2,10,12,14,1\n\
         Style: Sign,Georgia,30,&H0000FFFF,&H000000FF,&H00FF0000,&H00000000,-1,1,1,0,100,100,0,0,3,1,0,8,5,6,7,1\n"
    ))
}

/// `text` as a line of `script` in the style `style`, with margins of
/// its own.
fn line(script: &Script, style: &str, margins: [u32; 3], text: &str) -> Text {
    let [left, right, vertical] = margins;
    let source = format!("0,0,{style},,{left},{right},{vertical},,{text}");
    let dialogue = Dialogue::parse(&source);
    parse(dialogue.text, Some((script, &dialogue))).unwrap()
}

/// Each span's text and how it looks.
fn looks(text: &Text) -> Vec<(&str, Span)> {
    let mut at = 0;
    text.spans
        .iter()
        .map(|span| {
            let part = &text.text[at..at + span.len];
            at += span.len;
            (part, *span)
        })
        .collect()
}

fn look(text: &Text, at: usize) -> Look {
    text.spans[at].look.unwrap()
}

#[test]
fn reads_dialogue_lines() {
    let line = Dialogue::parse("12,0,Sign,Actor,0,15,0,,Hello, world");
    assert_eq!(
        line,
        Dialogue {
            style: "Sign",
            margins: [None, Some(15.0), None],
            text: "Hello, world",
        }
    );
    let line = Dialogue::parse("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hi, there");
    assert_eq!((line.style, line.text), ("Default", "Hi, there"));
    // Too few fields to be a line: all of it is its text.
    assert_eq!(Dialogue::parse("Just words").text, "Just words");
}

#[test]
fn a_scripts_header_sets_its_canvas_and_styles() {
    let script = script();
    assert_eq!(script.canvas, (640.0, 360.0));

    let text = line(&script, "Default", [0; 3], "Hello");
    let styled = text.styled.as_ref().unwrap();
    assert_eq!(
        (
            styled.canvas,
            styled.alignment,
            styled.margins,
            styled.position
        ),
        ((640.0, 360.0), 2, [10.0, 12.0, 14.0], None)
    );
    assert_eq!(
        (
            &*styled.font,
            styled.size,
            styled.outline,
            styled.shadow,
            styled.boxed
        ),
        ("Arial", 20.0, 2.0, 1.5, false)
    );
    assert_eq!(
        look(&text, 0),
        Look {
            fill: 0xffff_ffff,
            secondary: 0xff00_00ff,
            outline: 0x0000_00ff,
            // &H80: half see-through.
            shadow: 0x0000_007f,
            underline: false,
            strikeout: false,
            karaoke: None,
        }
    );
    assert!(!text.spans[0].italic && !text.spans[0].bold);
    assert_eq!(text.placement, Placement::Bottom);

    // Margins a line gives are its own, the others its style's.
    let sign = line(&script, "Sign", [0, 0, 40], "Sign");
    let styled = sign.styled.as_ref().unwrap();
    assert_eq!(
        (
            &*styled.font,
            styled.alignment,
            styled.margins,
            styled.boxed
        ),
        ("Georgia", 8, [5.0, 6.0, 40.0], true)
    );
    assert_eq!(sign.placement, Placement::Top);
    assert!(sign.spans[0].bold && sign.spans[0].italic);
    let look = look(&sign, 0);
    assert_eq!((look.fill, look.outline), (0xffff_00ff, 0x0000_ffff));
    assert!(look.underline && !look.strikeout);

    // A style is named in any case and with a leading `*`; one it
    // doesn't have is its Default.
    for (name, font, size) in [
        ("sign", "Georgia", 30.0),
        ("*Sign", "Georgia", 30.0),
        ("Missing", "Arial", 20.0),
    ] {
        let styled = line(&script, name, [0; 3], "x").styled.unwrap();
        assert_eq!((&*styled.font, styled.size), (font, size), "{name}");
    }
}

#[test]
fn a_header_without_sizes_or_styles_is_taken_as_libass_takes_it() {
    let canvas = |info: &str| Script::parse(&format!("[Script Info]\n{info}")).canvas;
    assert_eq!(canvas(""), (384.0, 288.0));
    assert_eq!(canvas("PlayResX: 640"), (640.0, 480.0));
    assert_eq!(canvas("PlayResX: 1280"), (1280.0, 1024.0));
    assert_eq!(canvas("PlayResY: 480"), (640.0, 480.0));
    assert_eq!(canvas("PlayResY: 1024"), (1280.0, 1024.0));
    assert_eq!(canvas("PlayResX: 0\nPlayResY: -5"), (384.0, 288.0));

    let script = Script::parse("");
    let text = line(&script, "Default", [0; 3], "x");
    let styled = text.styled.as_ref().unwrap();
    assert_eq!(
        (&*styled.font, styled.size, styled.alignment, styled.margins),
        ("Arial", 18.0, 2, [20.0; 3])
    );
    assert_eq!(look(&text, 0).secondary, 0x00ff_ffff);
}

#[test]
fn an_ssa_header_is_read_in_its_own_terms() {
    let script = Script::parse(
        "[Script Info]\nScriptType: v4.00\n\n[V4 Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, \
         BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, \
         MarginV, AlphaLevel, Encoding\n\
         Style: Default,Tahoma,24,255,65535,16711680,0,-1,0,1,2,1,6,10,10,10,0,0\n",
    );
    let text = line(&script, "Default", [0; 3], "x");
    let look = look(&text, 0);
    // Decimal colors, blue-green-red, opaque.
    assert_eq!(
        (look.fill, look.secondary, look.outline),
        (0xff00_00ff, 0xffff_00ff, 0x0000_ffff)
    );
    assert!(text.spans[0].bold);
    // SSA's 6 is the top center.
    assert_eq!(text.styled.unwrap().alignment, 8);

    let legacy = |n: u8| {
        let script = Script::parse(&format!(
            "[V4 Styles]\nFormat: Name, Alignment\nStyle: Default,{n}"
        ));
        line(&script, "Default", [0; 3], "x")
            .styled
            .unwrap()
            .alignment
    };
    let keypad: Vec<u8> = [1, 2, 3, 9, 10, 11, 5, 6, 7].map(legacy).into();
    assert_eq!(keypad, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    // Those SSA has no place for, as VSFilter takes them.
    assert_eq!((legacy(4), legacy(8)), (6, 3));
}

#[test]
fn override_tags_color_and_style_what_follows() {
    let script = script();
    let text = line(
        &script,
        "Default",
        [0; 3],
        r"A{\c&H0000FF&\3c&HFF0000&}B{\1a&H80&\3a&H80&\4a&HFF&}C{\alpha&H00&\c}D{\u1\s1\b1\i1}E{\r}F",
    );
    assert_eq!(text.text, "ABCDEF");
    let fills: Vec<(&str, u32, u32, u32)> = looks(&text)
        .iter()
        .map(|(part, span)| {
            let look = span.look.unwrap();
            (*part, look.fill, look.outline, look.shadow)
        })
        .collect();
    assert_eq!(
        fills,
        [
            ("A", 0xffff_ffff, 0x0000_00ff, 0x0000_007f),
            ("B", 0xff00_00ff, 0x0000_ffff, 0x0000_007f),
            // `\1a`, `\3a` and `\4a` set only the alpha.
            ("C", 0xff00_007f, 0x0000_ff7f, 0x0000_0000),
            // `\c` with no color is the style's, keeping the alpha.
            ("D", 0xffff_ffff, 0x0000_ffff, 0x0000_00ff),
            ("E", 0xffff_ffff, 0x0000_ffff, 0x0000_00ff),
            ("F", 0xffff_ffff, 0x0000_00ff, 0x0000_007f),
        ]
    );
    let e = text.spans[4];
    let e_look = e.look.unwrap();
    assert!(e.bold && e.italic && e_look.underline && e_look.strikeout);
    let f = text.spans[5];
    assert!(!f.bold && !f.italic && !f.look.unwrap().underline, "reset");

    // `\r` with a name is that style; with one the script hasn't, the
    // line's.
    let text = line(
        &script,
        "Default",
        [0; 3],
        r"A{\rSign}B{\c&H00FF00&\rNone}C",
    );
    let fills: Vec<u32> = text.spans.iter().map(|s| s.look.unwrap().fill).collect();
    assert_eq!(fills, [0xffff_ffff, 0xffff_00ff, 0xffff_ffff]);
    assert!(text.spans[1].bold && !text.spans[2].bold);
}

#[test]
fn the_first_place_a_line_is_given_wins() {
    let script = script();
    let placed = |source: &str| {
        let text = line(&script, "Default", [0; 3], source);
        let styled = text.styled.unwrap();
        (styled.alignment, styled.position, text.placement)
    };
    assert_eq!(
        placed(r"{\an7\an3\pos(10,20)\pos(30,40)}x"),
        (7, Some((10.0, 20.0)), Placement::Top)
    );
    assert_eq!(placed(r"{\an4}x{\an9}y"), (4, None, Placement::Middle));
    // A `\move` stays where it starts; `\a` counts SSA's way.
    assert_eq!(
        placed(r"{\move(5,6,100,200,0,500)\a10}x"),
        (5, Some((5.0, 6.0)), Placement::Middle)
    );
    // Out of range, it's the style's, and taken.
    assert_eq!(placed(r"{\an0\an8}x"), (2, None, Placement::Bottom));
    let sign = line(&script, "Sign", [0; 3], r"{\an10\an1}x");
    assert_eq!(sign.styled.unwrap().alignment, 8);
    // As are those SSA has no place for.
    for source in [r"{\a8}x", r"{\a4}x", r"{\a12}x"] {
        assert_eq!(placed(source), (7, None, Placement::Top), "{source}");
    }
    assert_eq!(placed(r"{\pos(1)}x"), (2, None, Placement::Bottom));
}

#[test]
fn a_line_is_sized_by_its_first_letter() {
    let script = script();
    let text = line(
        &script,
        "Default",
        [0; 3],
        r"{\fs10} {\fs30\bord4\shad0\fnHelvetica}Big{\fs5\bord1}small{\fs+2}",
    );
    let styled = text.styled.unwrap();
    assert_eq!(
        (&*styled.font, styled.size, styled.outline, styled.shadow),
        ("Helvetica", 30.0, 4.0, 0.0)
    );
    // Back to the style's with no value, and relative sizes aren't read.
    let text = line(&script, "Default", [0; 3], r"{\fs30\fs\bord\fn\fs+4}x");
    let styled = text.styled.unwrap();
    assert_eq!(
        (&*styled.font, styled.size, styled.outline),
        ("Arial", 20.0, 2.0)
    );
}

#[test]
fn karaoke_syllables_are_sung_in_turn() {
    let script = script();
    let text = line(
        &script,
        "Default",
        [0; 3],
        r"Hey {\k50}Ka{\K100}ra{\ko20}o{\kf40\2c&H00FF00&}ke",
    );
    let sung: Vec<(&str, Option<Karaoke>)> = looks(&text)
        .iter()
        .map(|(part, span)| (*part, span.look.unwrap().karaoke))
        .collect();
    let syllable = |at, outlined| Some(Karaoke { at, outlined });
    assert_eq!(
        sung,
        [
            ("Hey ", None),
            ("Ka", syllable(0, true)),
            // A sweep is sung halfway through.
            ("ra", syllable(1000, true)),
            ("o", syllable(1500, false)),
            ("ke", syllable(1900, true)),
        ]
    );
    assert_eq!(text.spans[4].look.unwrap().secondary, 0x00ff_00ff);
}

#[test]
fn tags_with_parentheses_and_those_not_read_are_passed_over() {
    let script = script();
    let text = line(
        &script,
        "Default",
        [0; 3],
        r"{\t(0,500,\c&H0000FF&\b1)\fscx120\frz10\xbord3\blur2\clip(0,0,5,5)\fad(10,20)}x{\b1}y",
    );
    assert_eq!(text.text, "xy");
    let x = text.spans[0];
    assert!(!x.bold && x.look.unwrap().fill == 0xffff_ffff);
    assert_eq!(
        text.styled.as_ref().unwrap().outline,
        2.0,
        "\\xbord isn't \\bord"
    );
    assert!(text.spans[1].bold);
}

#[test]
fn without_a_script_only_words_style_and_placement_are_kept() {
    let text = parse(r"{\c&H0000FF&\an8\pos(1,2)\k10}Hi", None).unwrap();
    assert_eq!(text.styled, None);
    assert_eq!(text.spans[0].look, None);
    assert_eq!(text.placement, Placement::Top);
}

#[test]
fn colors_read_as_ass_writes_them() {
    assert_eq!(parse_color("&H000000FF&"), Some(0xff00_00ff));
    assert_eq!(parse_color("&h80FF0000"), Some(0x0000_ff7f));
    assert_eq!(parse_color("&HFF&"), Some(0xff00_00ff));
    assert_eq!(parse_color("65280"), Some(0x00ff_00ff));
    assert_eq!(parse_color("&Hnope&"), None);
    assert_eq!(parse_color(""), None);
}
