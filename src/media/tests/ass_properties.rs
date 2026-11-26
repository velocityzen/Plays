use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;
use crate::media::subtitles::properties::assert_well_formed;

/// ASS dialogue text: override blocks of the tags read and some that
/// aren't, line breaks and hard spaces among any text, and braces left
/// open.
#[hegel::composite]
fn ass(tc: &TestCase) -> String {
    let piece = hegel::one_of!(
        gs::text(),
        gs::from_regex(
            r"\{(\\(i|b|u|s|r|p|an|a|c|1c|2c|3c|4c|alpha|1a|2a|3a|4a|k|K|kf|ko|bord|shad|fs|fn|fscx|t|xbord)[-+0-9&HA-Fa-z.]{0,9}|\\(pos|move|t|clip)\([-0-9,.]{0,12}\)?)*\}"
        ),
        gs::sampled_from(vec![r"\N", r"\n", r"\h", "{", "}", " "]).map(str::to_owned),
    );
    tc.draw(gs::vecs(piece).max_size(11)).concat()
}

/// A script's header: sections, sizes and styles, with any values.
#[hegel::composite]
fn header(tc: &TestCase) -> String {
    let line = hegel::one_of!(
        gs::sampled_from(vec![
            "[Script Info]",
            "[V4+ Styles]",
            "[V4 Styles]",
            "[Events]"
        ])
        .map(str::to_owned),
        gs::from_regex(r"PlayRes[XY]: ?-?[0-9]{0,12}(\.[0-9]{0,3})?"),
        gs::from_regex(
            r"Format: ?(Name|Fontname|Fontsize|Bold|Alignment|Outline|Shadow|MarginL|MarginV|BorderStyle|PrimaryColour)(, ?(Name|Fontsize|Bold|Alignment|Outline|Shadow|MarginR|MarginV|BorderStyle|BackColour)){0,6}"
        ),
        gs::from_regex(r"Style: ?(Default|Other|[a-z]{0,3})(,[-&Ha-fA-F0-9.]{0,12}){0,23}"),
        gs::text(),
    );
    tc.draw(gs::vecs(line).max_size(9)).join("\n")
}

/// Each character of `text` and whether it's italic and bold.
fn styled_chars(text: &Text) -> Vec<(char, bool, bool)> {
    let mut chars = Vec::new();
    let mut at = 0;
    for span in &text.spans {
        let part = &text.text[at..at + span.len];
        chars.extend(part.chars().map(|c| (c, span.italic, span.bold)));
        at += span.len;
    }
    chars
}

#[hegel::test]
fn any_dialogue_line_gives_text_it_ends_with(tc: TestCase) {
    let line = tc.draw(gs::text());
    assert!(line.ends_with(Dialogue::parse(&line).text));
}

#[hegel::test]
fn a_line_of_a_plain_script_reads_as_it_does_without_one(tc: TestCase) {
    let source = tc.draw(ass());
    let script = Script::parse("");
    let line = Dialogue::parse("0,0,Default,,0,0,0,,");
    let styled = parse(&source, Some((&script, &line)));
    let unstyled = parse(&source, None);
    assert_eq!(styled.is_some(), unstyled.is_some());
    let (Some(styled), Some(unstyled)) = (styled, unstyled) else {
        return;
    };
    assert_well_formed(&styled);
    assert_eq!(styled_chars(&styled), styled_chars(&unstyled));
    assert_eq!(styled.placement, unstyled.placement);
    assert!(styled.spans.iter().all(|span| span.look.is_some()));
    assert!(styled.styled.is_some() && unstyled.styled.is_none());
}

#[hegel::test]
fn karaoke_syllables_never_go_back(tc: TestCase) {
    let source = tc.draw(ass());
    let script = Script::parse("");
    let line = Dialogue::parse("0,0,Default,,0,0,0,,");
    let Some(text) = parse(&source, Some((&script, &line))) else {
        return;
    };
    let sung: Vec<u32> = text
        .spans
        .iter()
        .filter_map(|span| span.look?.karaoke)
        .map(|karaoke| karaoke.at)
        .collect();
    assert!(sung.is_sorted(), "{sung:?}");
}

#[hegel::test]
fn any_header_gives_a_canvas_and_styles_lines_can_be_drawn_in(tc: TestCase) {
    let script = Script::parse(&tc.draw(header()));
    let (width, height) = script.canvas;
    assert!(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0);
    let name = tc.draw(gs::sampled_from(vec!["Default", "Other", "missing"]));
    let source = format!("0,0,{name},,0,0,0,,");
    let line = Dialogue::parse(&source);
    let text = tc.draw(ass());
    if let Some(text) = parse(&format!("x{text}"), Some((&script, &line))) {
        let styled = text.styled.unwrap();
        assert!((1..=9).contains(&styled.alignment));
        assert!(styled.size > 0.0 && styled.size.is_finite());
        assert!(styled.outline >= 0.0 && styled.shadow >= 0.0);
        assert!(styled.margins.iter().all(|margin| *margin >= 0.0));
    }
}

#[hegel::test]
fn a_color_and_its_alpha_are_set_in_either_order(tc: TestCase) {
    let (rgb, alpha) = (
        tc.draw(gs::integers::<u32>().max_value(0xff_ffff)),
        tc.draw(gs::integers::<u8>()),
    );
    let [red, green, blue, _] = (rgb << 8).to_be_bytes();
    let color = format!(r"\c&H{blue:02X}{green:02X}{red:02X}&");
    let see_through = format!(r"\1a&H{alpha:02X}&");
    let script = Script::parse("");
    let line = Dialogue::parse("0,0,Default,,0,0,0,,");
    let fill = |tags: String| {
        let text = parse(&format!("{{{tags}}}x"), Some((&script, &line))).unwrap();
        text.spans[0].look.unwrap().fill
    };
    let expected = (rgb << 8) | u32::from(255 - alpha);
    assert_eq!(fill(format!("{color}{see_through}")), expected);
    assert_eq!(fill(format!("{see_through}{color}")), expected);
}
