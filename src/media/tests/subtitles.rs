use std::fs;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::files::test_folder;
use crate::media::external::SubtitleFile;

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
    let content = CueContent {
        texts: parse_plain("x").into_iter().collect(),
        bitmaps: Vec::new(),
    };
    Cue::new(start, end, Some(Arc::new(content)))
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
    queue.push(Cue::new(6.0, None, None));
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
    let content = CueContent {
        texts: parse_plain("y").into_iter().collect(),
        bitmaps: Vec::new(),
    };
    queue.push(Cue::new(1.0, Some(3.0), Some(Arc::new(content))));
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

/// A line of a script with no header of its own, as a cue from 1s to 5s.
fn karaoke(source: &str) -> Cue {
    let script = Script::parse("");
    let line = Dialogue::parse(source);
    let text = ass::parse(line.text, Some((&script, &line))).unwrap();
    let content = CueContent {
        texts: vec![text],
        bitmaps: Vec::new(),
    };
    Cue::new(1.0, Some(5.0), Some(Arc::new(content)))
}

/// The fill and outline of each part of the cue showing at `time`.
fn colors(queue: &mut CueQueue, time: f64) -> Vec<(u32, u32)> {
    queue.showing(time)[0].texts[0]
        .spans
        .iter()
        .map(|span| {
            let look = span.look.unwrap();
            (look.fill, look.outline)
        })
        .collect()
}

#[test]
fn karaoke_is_sung_as_its_cue_plays() {
    let mut queue = CueQueue::default();
    queue.push(karaoke(r"0,0,Default,,0,0,0,,{\k50}Ka{\ko100}ra"));
    let (white, cyan, black) = (0xffff_ffff, 0x00ff_ffff, 0x0000_00ff);
    // Not yet sung, in the secondary color; a `\ko` syllable without
    // its outline till then.
    assert_eq!(colors(&mut queue, 1.2), [(white, black), (cyan, 0)]);
    let first = queue.showing(1.2);
    let again = queue.showing(1.4);
    assert!(Arc::ptr_eq(&first[0], &again[0]), "the same till the next");
    assert_eq!(colors(&mut queue, 1.5), [(white, black), (white, black)]);
    assert!(!Arc::ptr_eq(&first[0], &queue.showing(1.5)[0]));
    assert!(
        queue.showing(1.5)[0].texts[0].spans.iter().all(|span| span
            .look
            .unwrap()
            .karaoke
            .is_none()),
        "as it shows, it's sung or not"
    );
    // Later by the delay too.
    queue.delay = 1.0;
    assert_eq!(colors(&mut queue, 2.2), [(white, black), (cyan, 0)]);
}

#[test]
fn a_cue_without_karaoke_shows_as_it_was_decoded() {
    let mut queue = CueQueue::default();
    let cue = karaoke(r"0,0,Default,,0,0,0,,{\c&H0000FF&}Plain");
    let content = cue.content.clone().unwrap();
    queue.push(cue);
    assert!(Arc::ptr_eq(&queue.showing(2.0)[0], &content));
}

/// The texts of the cues in a subtitle file named `name`, made of
/// `contents`.
fn decoded(name: &str, contents: &str) -> Vec<Text> {
    let folder = test_folder(name);
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    fs::write(&path, contents).unwrap();
    let (mut decoder, mut file) =
        SubtitleFile::open(&path, 0, None, &AtomicBool::new(false)).unwrap();
    let mut texts = Vec::new();
    while let Some(packet) = file.next(f64::INFINITY) {
        let content = decoder.decode(packet, 0.0).and_then(|cue| cue.content);
        texts.extend(content.iter().flat_map(|content| content.texts.clone()));
    }
    texts
}

#[test]
fn an_ass_files_lines_keep_how_its_script_draws_them() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\n\n\
                  [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, Alignment, \
                  MarginL, MarginR, MarginV\n\
                  Style: Default,Georgia,24,&H0000FFFF,2,10,10,10\n\
                  Style: Sign,Courier,30,&H00FF0000,8,10,10,25\n\n\
                  [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, \
                  Effect, Text\n\
                  Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hello {\\c&H0000FF&}red\n\
                  Dialogue: 0,0:00:03.00,0:00:04.00,Sign,,0,0,40,,{\\pos(320,40)}Sign\n";
    let texts = decoded("styled.ass", script);
    assert_eq!(texts.len(), 2, "{texts:?}");
    let fills: Vec<u32> = texts[0]
        .spans
        .iter()
        .map(|span| span.look.unwrap().fill)
        .collect();
    assert_eq!(texts[0].text, "Hello red");
    assert_eq!(fills, [0xffff_00ff, 0xff00_00ff]);
    let styled = texts[0].styled.as_ref().unwrap();
    assert_eq!(
        (styled.canvas, &*styled.font, styled.size, styled.alignment),
        ((640.0, 360.0), "Georgia", 24.0, 2)
    );
    let styled = texts[1].styled.as_ref().unwrap();
    assert_eq!(
        (
            &*styled.font,
            styled.alignment,
            styled.margins,
            styled.position
        ),
        ("Courier", 8, [10.0, 10.0, 40.0], Some((320.0, 40.0)))
    );

    // Another format's header is FFmpeg's own, so its lines are as the
    // settings style them.
    let srt = "1\n00:00:01,000 --> 00:00:02,000\n<font color=\"red\">Hi</font> <i>there</i>\n";
    let texts = decoded("plain.srt", srt);
    assert_eq!(texts[0].text, "Hi there");
    assert_eq!(texts[0].styled, None);
    assert!(texts[0].spans.iter().all(|span| span.look.is_none()));
    assert!(texts[0].spans.last().unwrap().italic);
}
