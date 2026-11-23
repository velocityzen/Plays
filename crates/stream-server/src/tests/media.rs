use std::path::PathBuf;
use std::sync::Arc;

use super::*;

/// A track named in Windows-1252, not UTF-8, as a Matroska file's may be,
/// named as it says.
#[test]
fn reads_a_track_named_in_another_encoding() {
    crate::init();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/western-tags.mkv");
    let source = Source::File(path);
    let input = open(&source, Arc::default()).unwrap();
    let info = MediaInfo::read(&input, &source.location());
    let titles: Vec<_> = info
        .streams
        .iter()
        .map(|stream| stream.title.as_deref())
        .collect();
    assert_eq!(titles, [Some("Français")]);
}
