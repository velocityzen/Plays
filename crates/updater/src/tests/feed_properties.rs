use std::io::Cursor;
use std::ops::Range;

use hegel::TestCase;
use hegel::generators as gs;

use super::tests::{APP, NOW, feed, key, keys, release};
use super::*;

/// Versions, newest first, none the same, all newer than 0.0.0.
#[hegel::composite]
fn versions(tc: &TestCase) -> Vec<Version> {
    let version = hegel::tuples!(
        gs::integers::<u64>().max_value(3),
        gs::integers::<u64>().max_value(3),
        gs::integers::<u64>().min_value(1).max_value(3)
    );
    let set = tc.draw(gs::btree_sets(version).min_size(1).max_size(6));
    set.into_iter()
        .rev()
        .map(|(major, minor, patch)| Version::new(major, minor, patch))
        .collect()
}

#[hegel::test]
fn the_releases_read_are_those_newer_than_the_running_one(tc: TestCase) {
    let versions = tc.draw(versions());
    let running = Version::new(
        tc.draw(gs::integers::<u64>().max_value(4)),
        tc.draw(gs::integers::<u64>().max_value(4)),
        tc.draw(gs::integers::<u64>().max_value(4)),
    );
    let key = key();
    let releases: Vec<Release> = versions
        .iter()
        .map(|version| release(&version.to_string()))
        .collect();
    let text = feed(&key, &releases);
    let read = read(Cursor::new(text), &keys(&key), APP, &running, NOW).unwrap();
    let newer: Vec<Release> = releases
        .into_iter()
        .filter(|release| release.version > running)
        .collect();
    match read {
        None => assert!(newer.is_empty()),
        Some(feed) => assert_eq!(feed.releases, newer),
    }
}

/// Where each signature's untrusted comment is in the feed `text`: from
/// where the signature starts to the end of its first line, which minisign
/// doesn't sign.
fn untrusted_comments(text: &str) -> Vec<Range<usize>> {
    let opening = "\"signature\":\"";
    text.match_indices(opening)
        .map(|(at, _)| {
            let start = at + opening.len();
            start..start + text[start..].find("\\n").expect("a signature's lines")
        })
        .collect()
}

#[hegel::test]
fn any_byte_changed_in_what_is_read_is_refused(tc: TestCase) {
    let versions = tc.draw(versions());
    let key = key();
    let releases: Vec<Release> = versions
        .iter()
        .map(|version| release(&version.to_string()))
        .collect();
    let text = feed(&key, &releases);
    // Any byte but those of an untrusted comment, and the last newline; as
    // it's older than every release, each line is read.
    let unsigned = untrusted_comments(&text);
    let signed: Vec<usize> = (0..text.len() - 1)
        .filter(|at| !unsigned.iter().any(|comment| comment.contains(at)))
        .collect();
    let at = signed[tc.draw(gs::integers::<usize>().max_value(signed.len() - 1))];
    let byte = tc.draw(gs::integers::<u8>());
    let mut text = text.into_bytes();
    if text[at] == byte {
        return;
    }
    text[at] = byte;
    let read = read(
        Cursor::new(text),
        &keys(&key),
        APP,
        &Version::new(0, 0, 0),
        NOW,
    );
    assert!(read.is_err(), "{read:?}");
}
