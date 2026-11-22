use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;

use super::*;

/// XML's pieces, put together any which way, among any characters.
fn xmlish() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        any::<char>().prop_map(String::from),
        select(vec![
            "<a>",
            "</a>",
            "<b x=\"1\"/>",
            "<p:c xmlns:p=\"u\" p:y='2'>",
            "</p:c>",
            "&amp;",
            "&#x41;",
            "&#xFFFFFFFF;",
            "&#0;",
            "&bogus;",
            "<![CDATA[<x>]]>",
            "<?xml version=\"1.0\"?>",
            "<!-- c -->",
            "<!DOCTYPE a>",
            "<",
            ">",
            "&",
            "\"",
            "\u{feff}",
        ])
        .prop_map(str::to_owned),
    ];
    vec(piece, 0..48).prop_map(|pieces| pieces.concat())
}

proptest! {
    #[test]
    fn any_text_is_read_or_refused_without_panicking(text in xmlish()) {
        if let Ok(root) = Element::parse(&text) {
            prop_assert!(depth(&root) <= MAX_DEPTH);
        }
    }

    #[test]
    fn elements_nest_only_so_deep(
        depth in prop_oneof![0..=2 * MAX_DEPTH, 0usize..100_000],
        empty in any::<bool>(),
    ) {
        let inside = if empty { "<b/>" } else { "" };
        let document = format!("{}{inside}{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let deepest = depth + usize::from(empty);
        prop_assert_eq!(
            Element::parse(&document).is_ok(),
            (1..=MAX_DEPTH).contains(&deepest)
        );
    }
}

/// How deep an element and what's in it go.
fn depth(element: &Element) -> usize {
    1 + element.children.iter().map(depth).max().unwrap_or(0)
}
