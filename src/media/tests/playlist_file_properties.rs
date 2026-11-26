use std::path::Path;

use hegel::TestCase;
use hegel::generators as gs;
use url::Url;

use super::*;

/// A name a playlist might list: no line break, nothing to trim, not a
/// comment, and no `=` for a `.pls` to split on.
#[hegel::composite]
fn name(tc: &TestCase) -> String {
    tc.draw(gs::from_regex(
        "[!\"$-<>-~][ -<>-~]{0,20}[!-<>-~]|[!\"$-<>-~]",
    ))
}

#[hegel::test]
fn any_text_lists_trimmed_entries_or_is_a_stream(tc: TestCase) {
    let text = tc.draw(gs::text());
    if let Some(listed) = entries(&text) {
        for entry in listed {
            assert!(!entry.is_empty());
            assert_eq!(entry.trim(), entry);
            assert!(!entry.contains('\n'));
        }
    }
}

#[hegel::test]
fn any_bytes_read_as_text(tc: TestCase) {
    let bytes = tc.draw(gs::binary());
    let read = text(&bytes);
    if let Ok(utf8) = std::str::from_utf8(&bytes)
        && encoding_rs::Encoding::for_bom(&bytes).is_none()
    {
        assert_eq!(read, utf8);
    }
}

#[hegel::test]
fn an_m3u_lists_what_was_written_in_it(tc: TestCase) {
    let names = tc.draw(gs::vecs(name()).max_size(8));
    let mut text = String::from("#EXTM3U\r\n");
    for name in &names {
        text.push_str("#EXTINF:-1,A title\r\n");
        text.push_str(name);
        text.push_str("\r\n");
    }
    assert_eq!(entries(&text).unwrap(), names);
}

#[hegel::test]
fn a_pls_lists_what_was_written_in_it_by_number(tc: TestCase) {
    let names = tc.draw(gs::vecs(name()).max_size(8));
    let mut lines: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(at, name)| format!("File{}={name}", at + 1))
        .collect();
    let shuffled = tc.draw(gs::booleans());
    if shuffled {
        lines.reverse();
    }
    let text = format!(
        "[playlist]\n{}\nNumberOfEntries={}\n",
        lines.join("\n"),
        names.len()
    );
    assert_eq!(entries(&text).unwrap(), names);
}

#[hegel::test]
fn any_entry_names_a_source_or_nothing_from_anywhere(tc: TestCase) {
    let entry = tc.draw(gs::text());
    if let Some(Source::File(path)) = resolve(&entry, &Base::Folder(Path::new("/lists"))) {
        assert!(path.is_absolute(), "{path:?}");
    }
    let base = Url::parse("https://radio.example/lists/all.m3u").unwrap();
    if let Some(Source::Url(url)) = resolve(&entry, &Base::Url(&base)) {
        assert!(Source::parse_url(&url).is_ok(), "{url}");
    }
}
