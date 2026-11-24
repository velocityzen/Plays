use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;

/// URLs' pieces, line breaks and spaces.
const PIECES: &[&str] = &[
    "http://",
    "HTTP://",
    "https://",
    "//",
    "/",
    "?",
    "#",
    ":",
    "[",
    "]",
    "::1",
    "@",
    "%0d%0a",
    "\r\n",
    "\n",
    " ",
    "\t",
    "é",
    "€",
    "192.0.2.10",
    "198.51.100.1",
    "80",
    "65536",
    "+1",
    "..",
    "./",
];

/// What a URL from a device might be: URLs' pieces put together any
/// which way, among any characters, line breaks and spaces.
#[hegel::composite]
pub(crate) fn url_text(tc: &TestCase) -> String {
    let piece = hegel::one_of!(
        gs::characters().map(String::from),
        gs::sampled_from(PIECES).map(str::to_owned),
    );
    tc.draw(gs::vecs(piece).max_size(15)).concat()
}

#[hegel::test]
#[hegel::explicit_test_case(text = String::new(), reference = "http://]::1//".to_owned())]
fn a_url_read_or_joined_goes_into_a_request_whole(tc: TestCase) {
    let text = tc.draw(url_text());
    let reference = tc.draw(url_text());
    let parsed = Url::parse(&text);
    let joined = parsed.as_ref().and_then(|url| url.join(&reference));
    let beside = Url::parse("http://192.0.2.10:9197/dmr/desc.xml")
        .unwrap()
        .join(&reference);
    for url in [parsed, joined, beside].into_iter().flatten() {
        assert!(is_clean(&url.host) && is_clean(&url.path), "{url:?}");
        assert!(url.path.starts_with('/'));
        assert_eq!(Url::parse(&url.to_string()), Some(url));
    }
}
