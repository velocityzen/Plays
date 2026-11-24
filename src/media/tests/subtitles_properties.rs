use proptest::prelude::*;

use super::*;

/// ASS dialogue text: override blocks, line breaks and hard spaces among
/// any text, and braces left open.
fn ass() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        any::<String>(),
        r"\{(\\(i|b|r|p|an|a)-?[0-9]{0,4})*\}",
        Just(r"\N".to_owned()),
        Just(r"\n".to_owned()),
        Just(r"\h".to_owned()),
        Just("{".to_owned()),
        Just("}".to_owned()),
    ];
    prop::collection::vec(piece, 0..12).prop_map(|pieces| pieces.concat())
}

/// `text` is lines, none blank or begun or ended blank, its spans covering
/// it, each a different style from the one before.
fn well_formed(text: &Text) -> Result<(), TestCaseError> {
    let mut at = 0;
    for span in &text.spans {
        prop_assert!(span.len > 0);
        at += span.len;
        prop_assert!(text.text.is_char_boundary(at));
    }
    prop_assert_eq!(at, text.text.len());
    for pair in text.spans.windows(2) {
        prop_assert!((pair[0].italic, pair[0].bold) != (pair[1].italic, pair[1].bold));
    }
    for line in text.text.split('\n') {
        prop_assert!(!line.is_empty());
        prop_assert_eq!(line.trim(), line);
    }
    Ok(())
}

/// A bitmap's width, height and stride, its palette indices, some past
/// the palette, and the palette.
fn bitmap() -> impl Strategy<Value = (usize, usize, usize, Vec<u8>, Vec<u32>)> {
    (1usize..12, 1usize..12, 0usize..4).prop_flat_map(|(width, height, pad)| {
        let stride = width + pad;
        (
            Just(width),
            Just(height),
            Just(stride),
            prop::collection::vec(0u8..12, stride * (height - 1) + width),
            prop::collection::vec(any::<u32>(), 0..8),
        )
    })
}

/// The words in `text`.
fn words(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

proptest! {
    #[test]
    fn any_dialogue_line_gives_text_it_ends_with(line in any::<String>()) {
        prop_assert!(line.ends_with(dialogue_text(&line)));
    }

    #[test]
    fn any_ass_text_reads_as_lines_its_spans_cover(source in ass()) {
        if let Some(text) = parse_ass(&source) {
            well_formed(&text)?;
        }
    }

    #[test]
    fn ass_text_with_nothing_to_read_in_it_reads_as_plain(source in r"[^{\\]*") {
        prop_assert_eq!(parse_ass(&source), parse_plain(&source));
    }

    #[test]
    fn plain_text_keeps_its_words_unstyled(source in any::<String>()) {
        match parse_plain(&source) {
            Some(text) => {
                well_formed(&text)?;
                prop_assert!(text.spans.iter().all(|span| !span.italic && !span.bold));
                prop_assert_eq!(words(&text.text), words(&source));
            }
            None => prop_assert!(source.trim().is_empty()),
        }
    }

    #[test]
    fn a_bitmaps_indices_become_its_palettes_colors(
        (width, height, stride, indices, palette) in bitmap(),
    ) {
        let made = pixels(&indices, stride, (width, height), &palette).unwrap();
        prop_assert_eq!(made.len(), width * height * 4);
        for (n, pixel) in made.chunks(4).enumerate() {
            let index = indices[n / width * stride + n % width];
            let color = palette.get(usize::from(index)).copied().unwrap_or(0);
            prop_assert_eq!(pixel, &color.to_le_bytes()[..]);
        }
    }

    #[test]
    fn any_bitmap_gives_whole_rows_or_none(
        indices in prop::collection::vec(any::<u8>(), 0..200),
        stride in 0usize..20,
        width in 0usize..20,
        height in 0usize..20,
        palette in prop::collection::vec(any::<u32>(), 0..4),
    ) {
        if let Some(made) = pixels(&indices, stride, (width, height), &palette) {
            prop_assert_eq!(made.len(), width * height * 4);
            prop_assert!(width > 0 && height > 0 && stride >= width);
        }
    }
}
