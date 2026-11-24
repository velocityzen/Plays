use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;

/// XML's pieces.
const PIECES: &[&str] = &[
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
];

/// XML's pieces, put together any which way, among any characters.
#[hegel::composite]
fn xmlish(tc: &TestCase) -> String {
    let piece = hegel::one_of!(
        gs::characters().map(String::from),
        gs::sampled_from(PIECES).map(str::to_owned),
    );
    tc.draw(gs::vecs(piece).max_size(47)).concat()
}

#[hegel::test]
fn any_text_is_read_or_refused_without_panicking(tc: TestCase) {
    let text = tc.draw(xmlish());
    if let Ok(root) = Element::parse(&text) {
        assert!(depth(&root) <= MAX_DEPTH);
    }
}

#[hegel::test]
fn elements_nest_only_so_deep(tc: TestCase) {
    let depth = tc.draw(hegel::one_of!(
        gs::integers::<usize>()
            .min_value(MAX_DEPTH - 1)
            .max_value(MAX_DEPTH + 1),
        gs::integers::<usize>().max_value(2 * MAX_DEPTH),
        gs::integers::<usize>().max_value(99_999),
    ));
    let empty = tc.draw(gs::booleans());
    let inside = if empty { "<b/>" } else { "" };
    let document = format!("{}{inside}{}", "<a>".repeat(depth), "</a>".repeat(depth));
    let deepest = depth + usize::from(empty);
    assert_eq!(
        Element::parse(&document).is_ok(),
        (1..=MAX_DEPTH).contains(&deepest)
    );
}

/// How deep an element and what's in it go.
fn depth(element: &Element) -> usize {
    1 + element.children.iter().map(depth).max().unwrap_or(0)
}
