use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;

use super::*;

/// What a URL from a device might be: URLs' pieces put together any
/// which way, among any characters, line breaks and spaces.
pub(crate) fn url_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        any::<char>().prop_map(String::from),
        select(vec![
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
        ])
        .prop_map(str::to_owned),
    ];
    vec(piece, 0..16).prop_map(|pieces| pieces.concat())
}

proptest! {
    #[test]
    fn a_url_read_or_joined_goes_into_a_request_whole(
        text in url_text(),
        reference in url_text(),
    ) {
        let parsed = Url::parse(&text);
        let joined = parsed.as_ref().and_then(|url| url.join(&reference));
        let beside = Url::parse("http://192.0.2.10:9197/dmr/desc.xml")
            .unwrap()
            .join(&reference);
        for url in [parsed, joined, beside].into_iter().flatten() {
            prop_assert!(is_clean(&url.host) && is_clean(&url.path), "{url:?}");
            prop_assert!(url.path.starts_with('/'));
            prop_assert_eq!(Url::parse(&url.to_string()), Some(url));
        }
    }
}
