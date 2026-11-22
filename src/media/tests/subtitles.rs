use super::*;

fn styled(text: &Text) -> Vec<(&str, bool, bool)> {
    let mut at = 0;
    text.spans
        .iter()
        .map(|span| {
            let part = &text.text[at..at + span.len];
            at += span.len;
            (part, span.italic, span.bold)
        })
        .collect()
}

#[test]
fn reads_dialogue_text() {
    assert_eq!(
        dialogue_text("12,0,Default,,0,0,0,,Hello, world"),
        "Hello, world"
    );
    assert_eq!(
        dialogue_text("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hi, there"),
        "Hi, there"
    );
}

#[test]
fn parses_ass_markup() {
    let text = parse_ass(r"{\i1}Hello{\i0}\Nworld\hagain").unwrap();
    assert_eq!(text.text, "Hello\nworld\u{a0}again");
    assert_eq!(
        styled(&text),
        [("Hello", true, false), ("\nworld\u{a0}again", false, false)]
    );
    assert_eq!(text.placement, Placement::Bottom);

    let text = parse_ass(r"{\an8\b1}Top{\b0} line").unwrap();
    assert_eq!(text.placement, Placement::Top);
    assert_eq!(
        styled(&text),
        [("Top", false, true), (" line", false, false)]
    );

    // Font tags and positions are ignored, drawings are skipped, blank
    // lines and spaces around lines are dropped.
    let text =
        parse_ass(r"{\fnArial\fs20\bord2\pos(10,20)} One \N\N{\p1}m 0 0 l 1 1{\p0}Two ").unwrap();
    assert_eq!(text.text, "One\nTwo");
    assert_eq!(parse_ass(r"{\p1}m 0 0 l 10 10"), None);
    assert_eq!(parse_ass(r" \N "), None);
    // An unclosed brace is just text.
    assert_eq!(parse_ass("a { b").unwrap().text, "a { b");
}

fn cue(start: f64, end: Option<f64>) -> Cue {
    Cue {
        start,
        end,
        content: Some(Arc::new(CueContent {
            texts: parse_plain("x").into_iter().collect(),
            bitmaps: Vec::new(),
        })),
    }
}

#[test]
fn shows_cues_in_their_time() {
    let mut queue = CueQueue::default();
    queue.push(cue(1.0, Some(3.0)));
    queue.push(cue(2.0, Some(4.0)));
    assert_eq!(queue.showing(0.5).len(), 0);
    assert_eq!(queue.showing(1.5).len(), 1);
    assert_eq!(queue.showing(2.5).len(), 2);
    assert_eq!(queue.showing(3.5).len(), 1);
    assert_eq!(queue.showing(4.0).len(), 0);
    assert_eq!(queue.cues.len(), 2, "kept a while, to go back to");
    assert_eq!(queue.showing(4.0 + KEEP).len(), 0);
    assert!(queue.cues.is_empty());
}

#[test]
fn shows_cues_later_or_earlier_by_the_delay() {
    let mut queue = CueQueue::default();
    queue.push(cue(1.0, Some(3.0)));
    queue.delay = 1.5;
    assert_eq!(queue.showing(2.0).len(), 0);
    assert_eq!(queue.showing(2.5).len(), 1);
    assert_eq!(queue.showing(4.4).len(), 1);
    assert_eq!(queue.showing(4.5).len(), 0);
    // Back earlier, it shows again: ended cues are kept a while.
    queue.delay = -0.5;
    assert_eq!(queue.showing(0.4).len(), 0);
    assert_eq!(queue.showing(0.5).len(), 1);
    assert_eq!(queue.showing(2.5).len(), 0);
    // Clearing for another track keeps the delay.
    queue.clear();
    assert_eq!(queue.delay, -0.5);
}

#[test]
fn open_cues_end_at_the_next_one() {
    let mut queue = CueQueue::default();
    queue.push(cue(1.0, None));
    assert_eq!(queue.showing(100.0).len(), 1);
    queue.clear();
    queue.push(cue(1.0, None));
    queue.push(cue(5.0, None));
    let first = queue.showing(2.0);
    assert_eq!(first.len(), 1);
    let second = queue.showing(5.0);
    assert_eq!(second.len(), 1);
    assert!(!Arc::ptr_eq(&first[0], &second[0]));
    // A cue with nothing in it clears the screen.
    queue.push(Cue {
        start: 6.0,
        end: None,
        content: None,
    });
    assert_eq!(queue.showing(5.5).len(), 1);
    assert_eq!(queue.showing(6.0).len(), 0);
}

#[test]
fn a_cue_decoded_again_is_kept_once() {
    let mut queue = CueQueue::default();
    queue.push(cue(1.0, Some(3.0)));
    queue.push(cue(1.0, Some(3.0)));
    assert_eq!(queue.showing(2.0).len(), 1);
    // Something else starting then is another cue.
    queue.push(Cue {
        start: 1.0,
        end: Some(3.0),
        content: Some(Arc::new(CueContent {
            texts: parse_plain("y").into_iter().collect(),
            bitmaps: Vec::new(),
        })),
    });
    assert_eq!(queue.showing(2.0).len(), 2);
}

#[test]
fn open_cues_go_on_only_as_far_as_the_playhead_saw() {
    let mut queue = CueQueue::default();
    queue.push(cue(5.0, None));
    // Back a frame from 6: still showing, and from where it started.
    queue.seeked(6.0, 5.96);
    assert_eq!(queue.showing(5.96).len(), 1);
    assert_eq!(queue.showing(5.0).len(), 1);
    // Read again from before it started, it is still one cue.
    queue.push(cue(5.0, None));
    assert_eq!(queue.showing(5.5).len(), 1);
    // Past where it was seen, it isn't known to show, so it doesn't.
    queue.seeked(6.0, 20.0);
    assert_eq!(queue.showing(20.0).len(), 0);
    assert_eq!(queue.showing(5.5).len(), 1);
}
