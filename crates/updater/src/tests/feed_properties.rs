use std::io::Cursor;

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

#[hegel::test]
fn any_byte_changed_in_what_is_read_is_refused(tc: TestCase) {
    let versions = tc.draw(versions());
    let key = key();
    let releases: Vec<Release> = versions
        .iter()
        .map(|version| release(&version.to_string()))
        .collect();
    let mut text = feed(&key, &releases).into_bytes();
    // Past the signature's line, whose untrusted comment isn't signed, and
    // older than every release, so each line is read.
    let start = text.iter().position(|&byte| byte == b'\n').unwrap() + 1;
    let at = tc.draw(
        gs::integers::<usize>()
            .min_value(start)
            .max_value(text.len() - 2),
    );
    let byte = tc.draw(gs::integers::<u8>());
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
