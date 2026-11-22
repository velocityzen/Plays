use super::*;

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
        },
        Cue {
            start: 3661.5,
            end: 3662.0,
            spans: vec![span("late", false, false)],
        },
    ];
    assert_eq!(
        webvtt(&cues, None, Some(900_000)),
        "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:900000,LOCAL:00:00:00.000\n\n\
             00:00:00.200 --> 00:00:01.000\nA &amp; B\n<i>leaning</i>\n\n\
             01:01:01.500 --> 01:01:02.000\nlate\n\n"
    );
    assert_eq!(webvtt(&cues, Some((1.0, 2.0)), None), "WEBVTT\n\n");
    assert_eq!(
        srt(&cues),
        "1\n00:00:00,200 --> 00:00:01,000\nA & B\n<i>leaning</i>\n\n\
             2\n01:01:01,500 --> 01:01:02,000\nlate\n\n"
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
