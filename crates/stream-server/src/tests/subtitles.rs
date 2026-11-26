use super::*;

use std::sync::atomic::AtomicBool;

const TOP_LEFT: Placement = Placement {
    vertical: Vertical::Top,
    horizontal: Horizontal::Left,
};
const MIDDLE_RIGHT: Placement = Placement {
    vertical: Vertical::Middle,
    horizontal: Horizontal::Right,
};
const TOP: Placement = Placement {
    vertical: Vertical::Top,
    horizontal: Horizontal::Center,
};

fn span(text: &str, italic: bool, bold: bool) -> Span {
    Span {
        text: text.into(),
        italic,
        bold,
    }
}

#[test]
fn reads_ass_styles_and_breaks() {
    assert_eq!(
        from_ass(r"{\i1}Hello{\i0}\Nworld\hagain"),
        vec![
            span("Hello", true, false),
            span("\nworld\u{a0}again", false, false)
        ]
    );
    assert_eq!(
        from_ass(r"{\b1\i1}both{\r}plain"),
        vec![span("both", true, true), span("plain", false, false)]
    );
    assert_eq!(
        from_ass(r"{\p1}m 0 0 l 10 10{\p0}text"),
        vec![span("text", false, false)]
    );
    assert_eq!(dialogue_text("0,0,Default,,0,0,0,,Hi, there"), "Hi, there");
}

#[test]
fn writes_webvtt_and_srt() {
    let cues = vec![
        Cue {
            start: 0.2,
            end: 1.0,
            spans: vec![span("A & B", false, false), span("\nleaning", true, false)],
            placement: Placement::default(),
        },
        Cue {
            start: 3661.5,
            end: 3662.0,
            spans: vec![span("late", false, false)],
            placement: TOP_LEFT,
        },
        Cue {
            start: 3663.0,
            end: 3664.0,
            spans: vec![span("aside", false, false)],
            placement: MIDDLE_RIGHT,
        },
    ];
    assert_eq!(
        webvtt(&cues, None, Some(900_000)),
        "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:900000,LOCAL:00:00:00.000\n\n\
             00:00:00.200 --> 00:00:01.000\nA &amp; B\n<i>leaning</i>\n\n\
             01:01:01.500 --> 01:01:02.000 line:0 position:5% align:left\nlate\n\n\
             01:01:03.000 --> 01:01:04.000 line:50%,center position:95% align:right\naside\n\n"
    );
    assert_eq!(webvtt(&cues, Some((1.0, 2.0)), None), "WEBVTT\n\n");
    assert_eq!(
        srt(&cues),
        "1\n00:00:00,200 --> 00:00:01,000\nA & B\n<i>leaning</i>\n\n\
             2\n01:01:01,500 --> 01:01:02,000\nlate\n\n\
             3\n01:01:03,000 --> 01:01:04,000\naside\n\n"
    );
}

#[test]
fn tidies_blank_lines_and_ends() {
    assert_eq!(
        tidy(vec![
            span(" one\r\n\r\n", false, false),
            span("two \n", true, false)
        ]),
        vec![span("one\n", false, false), span("two", true, false)]
    );
}

#[test]
fn places_cues_as_ass_and_ssa_align_them() {
    let at = |vertical, horizontal| Placement {
        vertical,
        horizontal,
    };
    use Horizontal::{Center, Left, Right};
    use Vertical::{Bottom, Middle, Top};
    let numpad = [
        at(Bottom, Left),
        at(Bottom, Center),
        at(Bottom, Right),
        at(Middle, Left),
        at(Middle, Center),
        at(Middle, Right),
        at(Top, Left),
        at(Top, Center),
        at(Top, Right),
    ];
    for (alignment, placement) in (1..).zip(numpad) {
        assert_eq!(Placement::numpad(alignment), Some(placement), "{alignment}");
    }
    assert_eq!(Placement::numpad(0), None);
    assert_eq!(Placement::numpad(10), None);
    let legacy = [
        (1, at(Bottom, Left)),
        (2, at(Bottom, Center)),
        (3, at(Bottom, Right)),
        (5, at(Top, Left)),
        (6, at(Top, Center)),
        (7, at(Top, Right)),
        (9, at(Middle, Left)),
        (10, at(Middle, Center)),
        (11, at(Middle, Right)),
    ];
    for (alignment, placement) in legacy {
        assert_eq!(Placement::legacy(alignment), Some(placement), "{alignment}");
    }
    for alignment in [0, 4, 8, 12, 13] {
        assert_eq!(Placement::legacy(alignment), None, "{alignment}");
    }
    let size = (300.0, 300.0);
    assert_eq!(Placement::at(0.0, 0.0, size), at(Top, Left));
    assert_eq!(Placement::at(150.0, 99.0, size), at(Top, Center));
    assert_eq!(Placement::at(150.0, 100.0, size), at(Middle, Center));
    assert_eq!(Placement::at(200.0, 200.0, size), at(Bottom, Right));
    assert_eq!(Placement::at(-10.0, 900.0, size), at(Bottom, Left));
}

const HEADER: &str = "[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, Alignment, MarginV
Style: Default,Arial,48,2,20
Style: Sign,Arial,48, 8 ,20
Style: Odd,Arial,48,12,20

[Events]
Format: Layer, Start, End, Style, Text
";

#[test]
fn reads_where_a_headers_styles_go_and_the_size_of_its_picture() {
    let styles = Styles::of(HEADER);
    assert_eq!(styles.size, (1920.0, 1080.0));
    assert_eq!(styles.placements.len(), 2, "{styles:?}");
    assert_eq!(styles.placements["Default"], Placement::default());
    assert_eq!(styles.placements["Sign"], TOP);

    // SSA's alignment, and the size players take when it isn't given.
    let ssa = "[V4 Styles]\nFormat: Name, Alignment\nStyle: Up, 6\n";
    let styles = Styles::of(ssa);
    assert_eq!(styles.placements["Up"], TOP);
    assert_eq!(styles.size, (384.0, 288.0));
    assert_eq!(Styles::of("PlayResX: 640").size, (640.0, 480.0));
    assert_eq!(Styles::of("PlayResY: 720").size, (960.0, 720.0));
    assert_eq!(Styles::of("PlayResX: 0\nPlayResY: 0").size, (384.0, 288.0));

    // Styles before the format of them, or without an alignment, say nothing.
    let unformatted = "[V4+ Styles]\nStyle: Sign,Arial,48,8,20\n";
    assert!(Styles::of(unformatted).placements.is_empty());
    let no_alignment = "[V4+ Styles]\nFormat: Name, Fontname\nStyle: Sign,Arial\n";
    assert!(Styles::of(no_alignment).placements.is_empty());
    // A section's format ends with it.
    let after = "[V4+ Styles]\nFormat: Name, Alignment\n[Fonts]\nStyle: Sign, 8\n";
    assert!(Styles::of(after).placements.is_empty());
}

#[test]
fn an_events_tags_place_it_over_its_style() {
    let styles = Styles::of(HEADER);
    let placement = |line: &str| styles.placement(line);
    assert_eq!(placement("0,0,Sign,,0,0,0,,Hi"), TOP);
    assert_eq!(placement("0,0,*Sign,,0,0,0,,Hi"), TOP);
    assert_eq!(
        placement("Dialogue: 0,0:00:00.00,0:00:01.00,Sign,,0,0,0,,Hi"),
        TOP
    );
    // An unknown style is the default.
    assert_eq!(placement("0,0,Nobody,,0,0,0,,Hi"), Placement::default());
    let raised = Styles::of("[V4+ Styles]\nFormat: Name, Alignment\nStyle: Default, 8\n");
    assert_eq!(raised.placement("0,0,Nobody,,0,0,0,,Hi"), TOP);
    assert_eq!(placement(r"0,0,Default,,0,0,0,,{\an7}Hi"), TOP_LEFT);
    assert_eq!(
        placement(r"0,0,Sign,,0,0,0,,{\an2}Hi"),
        Placement::default()
    );
    // SSA's, and the first of them.
    assert_eq!(placement(r"0,0,Default,,0,0,0,,{\a5}Hi{\an6}"), TOP_LEFT);
    assert_eq!(placement(r"0,0,Default,,0,0,0,,{\an6\an7}Hi"), MIDDLE_RIGHT);
    assert_eq!(placement(r"0,0,Default,,0,0,0,,{\a5\a11}Hi"), TOP_LEFT);
    // Colours and fades aren't alignments.
    assert_eq!(placement(r"0,0,Sign,,0,0,0,,{\alpha&H80&\a&H00&}Hi"), TOP);
    assert_eq!(
        placement(r"0,0,Default,,0,0,0,,{\a5\alpha&H80&}Hi"),
        TOP_LEFT
    );
    // Where it's put, on the header's picture, over any alignment.
    assert_eq!(
        placement(r"0,0,Default,,0,0,0,,{\an2\pos(1800,500)}Hi"),
        MIDDLE_RIGHT
    );
    assert_eq!(
        placement(r"0,0,Default,,0,0,0,,{\move(100,50,900,1000)}Hi"),
        TOP_LEFT
    );
    assert_eq!(placement(r"0,0,Sign,,0,0,0,,{\pos(1800, x)}Hi"), TOP);
    assert_eq!(
        placement(r"0,0,Default,,0,0,0,,{\pos(100,50)}{\pos(1800,500)}Hi"),
        TOP_LEFT
    );
    assert_eq!(
        placement(r"0,0,Default,,0,0,0,,{\an9}{\an1}Hi"),
        Placement {
            vertical: Vertical::Top,
            horizontal: Horizontal::Right,
        }
    );
    // An unclosed block says nothing.
    assert_eq!(
        placement(r"0,0,Default,,0,0,0,,{\an8 Hi"),
        Placement::default()
    );
}

/// A subtitle file, `name` in a folder of the test's own.
fn written(test: &str, name: &str, text: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir()
        .join("plays-stream-server-tests")
        .join(test);
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn cues_of(path: std::path::PathBuf) -> Vec<Cue> {
    let subtitles = SubtitleSource::File {
        path,
        encoding: None,
    };
    let media = Source::File(std::path::PathBuf::from("unused.mp4"));
    read(&subtitles, &media, Arc::new(AtomicBool::new(false))).unwrap()
}

#[test]
fn an_srt_cue_goes_where_its_tag_puts_it() {
    let srt = "1\n00:00:00,200 --> 00:00:01,000\n{\\an8}Up\n\n\
               2\n00:00:01,200 --> 00:00:01,900\nDown\n\n";
    let cues = cues_of(written("srt-placement", "tagged.srt", srt));
    let placements: Vec<_> = cues.iter().map(|cue| cue.placement).collect();
    assert_eq!(placements, [TOP, Placement::default()]);
    assert_eq!(cues[0].spans, [span("Up", false, false)]);
}

#[test]
fn an_ass_cue_goes_where_its_style_or_tags_put_it() {
    let ass = format!(
        "{HEADER}Dialogue: 0,0:00:00.20,0:00:01.00,Sign,Up\n\
         Dialogue: 0,0:00:01.20,0:00:01.90,Default,{{\\pos(100,540)}}Aside\n\
         Dialogue: 0,0:00:02.00,0:00:02.50,Default,Down\n"
    );
    let cues = cues_of(written("ass-placement", "styled.ass", &ass));
    let placements: Vec<_> = cues.iter().map(|cue| cue.placement).collect();
    assert_eq!(
        placements,
        [
            TOP,
            Placement {
                vertical: Vertical::Middle,
                horizontal: Horizontal::Left,
            },
            Placement::default()
        ],
        "{cues:?}"
    );
}
