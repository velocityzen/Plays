use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;

/// ASS dialogue text: override blocks, line breaks and hard spaces among
/// any text, and braces left open.
#[hegel::composite]
fn ass(tc: &TestCase) -> String {
    let piece = hegel::one_of!(
        gs::text(),
        gs::from_regex(r"\{(\\(i|b|r|p|an|a)-?[0-9]{0,4})*\}"),
        gs::sampled_from(vec![r"\N", r"\n", r"\h", "{", "}"]).map(str::to_owned),
    );
    tc.draw(gs::vecs(piece).max_size(11)).concat()
}

/// `text` is lines, none blank or begun or ended blank, its spans covering
/// it, each a different style from the one before.
fn assert_well_formed(text: &Text) {
    let mut at = 0;
    for span in &text.spans {
        assert!(span.len > 0);
        at += span.len;
        assert!(text.text.is_char_boundary(at));
    }
    assert_eq!(at, text.text.len());
    for pair in text.spans.windows(2) {
        assert!((pair[0].italic, pair[0].bold) != (pair[1].italic, pair[1].bold));
    }
    for line in text.text.split('\n') {
        assert!(!line.is_empty());
        assert_eq!(line.trim(), line);
    }
}

/// A bitmap's width, height and stride, its palette indices, some past
/// the palette, and the palette.
#[hegel::composite]
fn bitmap(tc: &TestCase) -> (usize, usize, usize, Vec<u8>, Vec<u32>) {
    let width = tc.draw(gs::integers::<usize>().min_value(1).max_value(11));
    let height = tc.draw(gs::integers::<usize>().min_value(1).max_value(11));
    let stride = width + tc.draw(gs::integers::<usize>().max_value(3));
    let count = stride * (height - 1) + width;
    let indices = tc.draw(
        gs::vecs(gs::integers::<u8>().max_value(11))
            .min_size(count)
            .max_size(count),
    );
    let palette = tc.draw(gs::vecs(gs::integers::<u32>()).max_size(7));
    (width, height, stride, indices, palette)
}

/// The words in `text`.
fn words(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

#[hegel::test]
fn any_dialogue_line_gives_text_it_ends_with(tc: TestCase) {
    let line = tc.draw(gs::text());
    assert!(line.ends_with(dialogue_text(&line)));
}

#[hegel::test]
fn any_ass_text_reads_as_lines_its_spans_cover(tc: TestCase) {
    if let Some(text) = parse_ass(&tc.draw(ass())) {
        assert_well_formed(&text);
    }
}

#[hegel::test]
fn ass_text_with_nothing_to_read_in_it_reads_as_plain(tc: TestCase) {
    let source = tc.draw(gs::from_regex(r"[^{\\]*"));
    assert_eq!(parse_ass(&source), parse_plain(&source));
}

#[hegel::test]
fn plain_text_keeps_its_words_unstyled(tc: TestCase) {
    let source = tc.draw(gs::text());
    match parse_plain(&source) {
        Some(text) => {
            assert_well_formed(&text);
            assert!(text.spans.iter().all(|span| !span.italic && !span.bold));
            assert_eq!(words(&text.text), words(&source));
        }
        None => assert!(source.trim().is_empty()),
    }
}

#[hegel::test]
fn a_bitmaps_indices_become_its_palettes_colors(tc: TestCase) {
    let (width, height, stride, indices, palette) = tc.draw(bitmap());
    let made = pixels(&indices, stride, (width, height), &palette).unwrap();
    assert_eq!(made.len(), width * height * 4);
    for (n, pixel) in made.chunks(4).enumerate() {
        let index = indices[n / width * stride + n % width];
        let color = palette.get(usize::from(index)).copied().unwrap_or(0);
        assert_eq!(pixel, &color.to_le_bytes()[..]);
    }
}

#[hegel::test]
fn any_bitmap_gives_whole_rows_or_none(tc: TestCase) {
    let width = tc.draw(gs::integers::<usize>().max_value(19));
    let height = tc.draw(gs::integers::<usize>().max_value(19));
    // Often about as wide as the bitmap, and as many indices as it needs.
    let stride = tc.draw(hegel::one_of!(
        gs::integers::<usize>().max_value(19),
        gs::integers::<usize>()
            .min_value(width.saturating_sub(1))
            .max_value(width + 1),
    ));
    let needed = stride * height.saturating_sub(1) + width;
    let count = tc.draw(hegel::one_of!(
        gs::integers::<usize>().max_value(199),
        gs::integers::<usize>()
            .min_value(needed.saturating_sub(1))
            .max_value(needed + 1),
    ));
    let indices = tc.draw(gs::binary().min_size(count).max_size(count));
    let palette = tc.draw(gs::vecs(gs::integers::<u32>()).max_size(3));
    if let Some(made) = pixels(&indices, stride, (width, height), &palette) {
        assert_eq!(made.len(), width * height * 4);
        assert!(width > 0 && height > 0 && stride >= width);
    }
}
