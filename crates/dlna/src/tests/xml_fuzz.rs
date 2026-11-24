use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// XML's pieces, right and wrong, among any characters.
const PIECES: &[&str] = &[
    "<a>",
    "</a>",
    "</b>",
    "<b x=\"1\"/>",
    "<b x='1' x='2'/>",
    "<b x/>",
    "<b x=1>",
    "<p:c xmlns:p=\"u\" p:y='&lt;'>",
    "</p:c>",
    "&amp;",
    "&#x41;",
    "&#xD800;",
    "&#x110000;",
    "&#0;",
    "&bogus;",
    "<![CDATA[<x>]]>",
    "<![CDATA[",
    "]]>",
    "<?xml version=\"1.0\"?>",
    "<?pi",
    "<!-- c -->",
    "<!--",
    "<!DOCTYPE a [<!ENTITY e \"x\">]>",
    "<",
    ">",
    "&",
    "\"",
    "\r\n",
    "\u{feff}",
];

#[hegel::test]
fn any_text_is_read_or_refused_and_nests_only_so_deep(tc: TestCase) {
    let mut text = String::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(48)) {
        if tc.draw(gs::weighted_booleans(0.8)) {
            text.push_str(tc.draw(gs::sampled_from(PIECES)));
        } else {
            text.push(tc.draw(gs::characters()));
        }
    }
    if let Ok(root) = Element::parse(&text) {
        assert!(depth(&root) <= MAX_DEPTH);
    }
}

/// How deep an element and what's in it go.
fn depth(element: &Element) -> usize {
    1 + element.children.iter().map(depth).max().unwrap_or(0)
}

#[hegel::test]
fn an_element_written_reads_back_as_it_was(tc: TestCase) {
    let prolog = tc.draw(gs::sampled_from(vec![
        "",
        "\u{feff}",
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
        "<!-- a -->",
    ]));
    let (element, written) = tree(&tc, 3);
    assert_eq!(Element::parse(&format!("{prolog}{written}")), Ok(element));
}

/// An element, with or without a prefix, its attributes, and text between
/// its children, `depth` deep at most; and it written, escaped, as a
/// device would.
fn tree(tc: &TestCase, depth: usize) -> (Element, String) {
    let name = tc.draw(gs::sampled_from(vec!["a", "item", "s:Body", "u:x"]));
    let mut element = Element {
        name: local(name),
        ..Element::default()
    };
    let mut written = format!("<{name}");
    for attribute in tc.draw(gs::subsequences(vec!["id", "s:x", "protocolInfo", "u:y"])) {
        // XML reads a line break or a tab in an attribute as a space.
        let value = tc.draw(gs::text().max_size(8).exclude_characters("\t\n\r"));
        let quote = tc.draw(gs::sampled_from(vec!['"', '\'']));
        written.push_str(&format!(" {attribute}={quote}{}{quote}", escape(&value)));
        element.attributes.push((local(attribute), value));
    }
    let children = if depth == 0 {
        0
    } else {
        tc.draw(gs::integers::<usize>().max_value(3))
    };
    let mut inside = String::new();
    for i in 0..=children {
        // XML reads a carriage return in text as a line feed.
        let text = tc.draw(gs::text().max_size(8).exclude_characters("\r"));
        if !text.contains("]]>") && tc.draw(gs::booleans()) {
            inside.push_str(&format!("<![CDATA[{text}]]>"));
        } else {
            inside.push_str(&escape(&text));
        }
        element.text.push_str(&text);
        if i < children {
            let (child, child_written) = tree(tc, depth - 1);
            inside.push_str(&child_written);
            element.children.push(child);
        }
    }
    if inside.is_empty() && tc.draw(gs::booleans()) {
        written.push_str("/>");
    } else {
        written.push_str(&format!(">{inside}</{name}>"));
    }
    (element, written)
}

/// A name without its prefix.
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}
