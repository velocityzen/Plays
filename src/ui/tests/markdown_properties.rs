use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// Every run and link in `text` is in it, on characters, in order.
fn holds(text: &Text) {
    let mut after = 0;
    for (range, _) in &text.runs {
        assert!(
            range.start >= after && range.start < range.end && range.end <= text.text.len(),
            "{text:?}"
        );
        assert!(
            text.text.is_char_boundary(range.start) && text.text.is_char_boundary(range.end),
            "{text:?}"
        );
        after = range.end;
    }
    let mut after = 0;
    for (range, _) in &text.links {
        assert!(
            range.start >= after && range.start < range.end && range.end <= text.text.len(),
            "{text:?}"
        );
        after = range.end;
    }
}

#[hegel::test]
fn whatever_is_written_parses_to_runs_in_its_text(tc: TestCase) {
    let markdown = tc.draw(
        gs::text()
            .alphabet("ab é*_~`[]()#>-1. \n!<>/")
            .max_size(200),
    );
    for block in parse(&markdown) {
        match block {
            Block::Heading(level, text) => {
                assert!((1..=6).contains(&level));
                holds(&text);
            }
            Block::Paragraph(text) | Block::Quote(text) | Block::Item { text, .. } => {
                assert!(!text.text.is_empty());
                holds(&text);
            }
            Block::Code(_) | Block::Rule => {}
        }
    }
}
