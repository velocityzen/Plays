use super::*;

fn plain(text: &str) -> Text {
    Text {
        text: text.into(),
        ..Text::default()
    }
}

/// The one paragraph `markdown` is.
fn paragraph(markdown: &str) -> Text {
    match parse(markdown).as_slice() {
        [Block::Paragraph(text)] => text.clone(),
        blocks => panic!("{blocks:?}"),
    }
}

fn item(depth: usize, number: Option<u64>, text: &str) -> Block {
    Block::Item {
        depth,
        number,
        text: plain(text),
    }
}

#[test]
fn headings_and_paragraphs() {
    let blocks = parse("# Plays 0.2.0\n\nIt plays more.\nAnd better.\n\n### Fixed");
    assert_eq!(
        blocks,
        [
            Block::Heading(1, plain("Plays 0.2.0")),
            Block::Paragraph(plain("It plays more. And better.")),
            Block::Heading(3, plain("Fixed")),
        ]
    );
}

#[test]
fn runs_are_styled_as_written() {
    let text = paragraph("A **strong** and *em* ~~gone~~ `code` ***both***");
    assert_eq!(text.text, "A strong and em gone code both");
    let look = |at: &str| {
        let start = text.text.find(at).unwrap();
        let run = text
            .runs
            .iter()
            .find(|(range, _)| range.start == start)
            .unwrap();
        assert_eq!(&text.text[run.0.clone()], at);
        run.1
    };
    assert_eq!(
        look("strong"),
        Look {
            strong: true,
            ..Look::default()
        }
    );
    assert_eq!(
        look("em"),
        Look {
            emphasis: true,
            ..Look::default()
        }
    );
    assert_eq!(
        look("gone"),
        Look {
            struck: true,
            ..Look::default()
        }
    );
    assert_eq!(
        look("code"),
        Look {
            code: true,
            ..Look::default()
        }
    );
    assert_eq!(
        look("both"),
        Look {
            strong: true,
            emphasis: true,
            ..Look::default()
        }
    );
    assert_eq!(text.runs.len(), 5);
}

#[test]
fn links_open_where_they_go() {
    let text = paragraph("See [the **notes**](https://example.com/notes) now.");
    assert_eq!(text.text, "See the notes now.");
    assert_eq!(
        text.links,
        [(4..13, "https://example.com/notes".to_owned())]
    );
    let link = Look {
        link: true,
        ..Look::default()
    };
    assert_eq!(
        text.runs,
        [
            (4..8, link),
            (
                8..13,
                Look {
                    strong: true,
                    ..link
                }
            )
        ]
    );
}

#[test]
fn lists_go_in_and_are_numbered() {
    let blocks = parse("- one\n- two\n  1. first\n  2. second\n- three\n\n3. three\n4. four");
    assert_eq!(
        blocks,
        [
            item(0, None, "one"),
            item(0, None, "two"),
            item(1, Some(1), "first"),
            item(1, Some(2), "second"),
            item(0, None, "three"),
            item(0, Some(3), "three"),
            item(0, Some(4), "four"),
        ]
    );
}

#[test]
fn an_item_of_paragraphs_is_one_item() {
    let blocks = parse("- one\n\n  more of it\n- two");
    assert_eq!(
        blocks,
        [item(0, None, "one\nmore of it"), item(0, None, "two")]
    );
}

#[test]
fn quotes_code_and_rules() {
    let blocks = parse("> Said\n> again\n\n```\nlet x = 1;\n\n```\n\n---\n\nAfter");
    assert_eq!(
        blocks,
        [
            Block::Quote(plain("Said again")),
            Block::Code("let x = 1;".into()),
            Block::Rule,
            Block::Paragraph(plain("After")),
        ]
    );
}

#[test]
fn html_is_left_out_and_an_image_is_its_text() {
    let blocks = parse("<div>hidden</div>\n\nA ![picture](a.png) <b>here</b>");
    assert_eq!(blocks, [Block::Paragraph(plain("A picture here"))]);
}

#[test]
fn nothing_is_no_blocks() {
    assert!(parse("").is_empty());
    assert!(parse("\n\n   \n").is_empty());
}
