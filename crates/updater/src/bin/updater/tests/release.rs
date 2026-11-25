use minisign::KeyPair;

use super::*;
use crate::tests::{Scratch, installs};

/// When the feeds here are made.
const NOW: u64 = 1_800_000_000;

fn key() -> Key {
    let pair = KeyPair::generate_unencrypted_keypair().unwrap();
    Key {
        secret: pair.sk,
        public: pair.pk,
    }
}

fn release(version: &str) -> Release {
    let whole = File {
        url: format!("{version}.update"),
        size: 10,
        sha256: hash::of(version.as_bytes()),
    };
    Release {
        version: Version::parse(version).unwrap(),
        published: NOW,
        notes: format!("## {version}"),
        downloads: BTreeMap::from([(
            "macos-aarch64".to_owned(),
            Download {
                whole,
                deltas: BTreeMap::new(),
            },
        )]),
    }
}

/// The versions in the feed `text`, read at `now`.
fn versions(text: &str, key: &Key, now: u64) -> Vec<String> {
    let releases = read(text, &key.keys().unwrap(), now).unwrap();
    releases
        .iter()
        .map(|release| release.version.to_string())
        .collect()
}

fn new_feed(key: &Key, days: u64) -> String {
    add(None, &release("0.1.0"), Some("plays"), key, NOW, days).unwrap()
}

#[test]
fn a_new_feed_has_the_release_for_the_app_for_as_many_days_as_said() {
    let key = key();
    let text = new_feed(&key, 30);
    assert_eq!(versions(&text, &key, NOW), ["0.1.0"]);
    let header = header(&text).unwrap();
    assert_eq!(
        (header.app.as_str(), header.published, header.expires),
        ("plays", NOW, NOW + 30 * DAY)
    );
}

#[test]
fn a_release_goes_first_and_the_earlier_lines_stay_as_they_were() {
    let key = key();
    let first = new_feed(&key, 30);
    let second = add(Some(&first), &release("0.2.0"), None, &key, NOW + DAY, 30).unwrap();
    assert_eq!(versions(&second, &key, NOW + DAY), ["0.2.0", "0.1.0"]);
    assert_eq!(
        feed::release_lines(&second)[1..],
        feed::release_lines(&first)[..]
    );
}

#[test]
fn a_release_no_newer_than_the_latest_isnt_added() {
    let key = key();
    let text = add(
        Some(&new_feed(&key, 30)),
        &release("0.2.0"),
        None,
        &key,
        NOW,
        30,
    )
    .unwrap();
    for version in ["0.2.0", "0.1.5", "0.2.0-rc.1"] {
        let added = add(Some(&text), &release(version), None, &key, NOW, 30);
        assert_eq!(
            added.unwrap_err(),
            format!("{version} isn't newer than the feed's latest, 0.2.0")
        );
    }
}

#[test]
fn a_new_feed_needs_its_app_and_one_there_keeps_its_own() {
    let key = key();
    assert!(add(None, &release("0.1.0"), None, &key, NOW, 30).is_err());
    let text = new_feed(&key, 30);
    let other = add(Some(&text), &release("0.2.0"), Some("other"), &key, NOW, 30);
    assert_eq!(other.unwrap_err(), "the feed is plays's, not other's");
    assert!(add(Some(&text), &release("0.2.0"), Some("plays"), &key, NOW, 30).is_ok());
}

#[test]
fn a_feed_whose_releases_dont_hold_isnt_signed() {
    let key = key();
    let text = add(
        Some(&new_feed(&key, 30)),
        &release("0.2.0"),
        None,
        &key,
        NOW,
        30,
    )
    .unwrap();
    let changed = text.replacen("## 0.1.0", "## 0.1.1", 1);
    assert!(add(Some(&changed), &release("0.3.0"), None, &key, NOW, 30).is_err());
    assert!(again(&changed, &key, NOW, 30).is_err());
}

#[test]
fn a_feed_signed_again_is_taken_for_longer_its_releases_as_they_were() {
    let key = key();
    let text = new_feed(&key, 1);
    let later = NOW + 2 * DAY;
    assert_eq!(
        read(&text, &key.keys().unwrap(), later).unwrap_err(),
        "the feed is out of date"
    );
    let signed = again(&text, &key, later, 30).unwrap();
    assert_eq!(versions(&signed, &key, later), ["0.1.0"]);
    assert_eq!(feed::release_lines(&signed), feed::release_lines(&text));
}

/// In `scratch`, beside `installs`' update files: the delta from 0.1.0
/// to 0.2.0, and 0.2.0's notes.
fn release_files(scratch: &Scratch) {
    installs(scratch);
    let base = Some((scratch.join("old"), "0.1.0"));
    let base = base.as_ref().map(|(path, from)| (path.as_path(), *from));
    package::make(
        &scratch.join("new"),
        base,
        "0.2.0",
        3,
        &scratch.join("0.1.0-0.2.0.update"),
    )
    .unwrap();
    fs::write(scratch.join("notes.md"), "## 0.2.0\n\n- Plays more.\n\n").unwrap();
}

fn added(notes: &Path, updates: &[String], deltas: &[String]) -> Result<Release> {
    Added {
        notes,
        updates,
        deltas,
        url: Some("https://example.com/plays/"),
        app: None,
    }
    .release(NOW)
}

#[test]
fn a_release_is_of_its_update_files() {
    let scratch = Scratch::new();
    release_files(&scratch);
    let updates = [
        format!("macos-aarch64={}", scratch.arg("0.2.0.update")),
        format!("linux-x86_64={}", scratch.arg("0.2.0.update")),
    ];
    let deltas = [format!(
        "macos-aarch64={}",
        scratch.arg("0.1.0-0.2.0.update")
    )];
    let release = added(&scratch.join("notes.md"), &updates, &deltas).unwrap();
    assert_eq!(release.version, Version::new(0, 2, 0));
    assert_eq!(release.published, NOW);
    assert_eq!(release.notes, "## 0.2.0\n\n- Plays more.");
    let mac = &release.downloads["macos-aarch64"];
    let whole = scratch.join("0.2.0.update");
    assert_eq!(mac.whole.url, "https://example.com/plays/0.2.0.update");
    assert_eq!(mac.whole.size, fs::metadata(&whole).unwrap().len());
    assert_eq!(mac.whole.sha256, hash::of_file(&whole).unwrap());
    assert_eq!(mac.deltas.keys().collect::<Vec<_>>(), ["0.1.0"]);
    assert_eq!(
        mac.deltas["0.1.0"].url,
        "https://example.com/plays/0.1.0-0.2.0.update"
    );
    assert!(release.downloads["linux-x86_64"].deltas.is_empty());
}

#[test]
fn update_files_that_dont_make_a_release_say_why() {
    let scratch = Scratch::new();
    release_files(&scratch);
    let notes = scratch.join("notes.md");
    let whole = |platform: &str, version: &str| {
        format!("{platform}={}", scratch.arg(&format!("{version}.update")))
    };
    let delta = format!("macos-aarch64={}", scratch.arg("0.1.0-0.2.0.update"));
    let refused = [
        (vec![], vec![], "a release needs an update".to_owned()),
        (
            vec!["macos-aarch64".to_owned()],
            vec![],
            "macos-aarch64 isn't PLATFORM=FILE".to_owned(),
        ),
        (
            vec![
                whole("macos-aarch64", "0.2.0"),
                whole("linux-x86_64", "0.1.0"),
            ],
            vec![],
            format!(
                "{} is 0.1.0, and the others aren't",
                scratch.join("0.1.0.update").display()
            ),
        ),
        (
            vec![
                whole("macos-aarch64", "0.2.0"),
                whole("macos-aarch64", "0.2.0"),
            ],
            vec![],
            "macos-aarch64 has two updates".to_owned(),
        ),
        (
            vec![delta.clone()],
            vec![],
            format!(
                "{} is a delta from 0.1.0, not a whole update",
                scratch.join("0.1.0-0.2.0.update").display()
            ),
        ),
        (
            vec![whole("macos-aarch64", "0.2.0")],
            vec![whole("macos-aarch64", "0.1.0")],
            format!(
                "{} is a whole update, not a delta",
                scratch.join("0.1.0.update").display()
            ),
        ),
        (
            vec![whole("linux-x86_64", "0.2.0")],
            vec![delta.clone()],
            "macos-aarch64 has a delta, and no whole update".to_owned(),
        ),
    ];
    for (updates, deltas, why) in refused {
        let release = added(&notes, &updates, &deltas);
        assert_eq!(release.unwrap_err(), why, "{updates:?} {deltas:?}");
    }
}

#[test]
fn a_feed_taken_for_fewer_days_than_asked_is_to_be_signed_again() {
    let header = header(&new_feed(&key(), 30)).unwrap();
    assert_eq!(days_left(&header, NOW, 30), Ok(30));
    assert_eq!(days_left(&header, NOW + 20 * DAY, 10), Ok(10));
    assert_eq!(
        days_left(&header, NOW + 20 * DAY + 1, 10),
        Err("it's taken for 9 days more, fewer than 10: sign it again".into())
    );
}

#[test]
fn a_feed_is_verified_with_any_of_the_keys_given() {
    let scratch = Scratch::new();
    let (signer, other) = (key(), key());
    let text = add(None, &release("0.1.0"), Some("plays"), &signer, now(), 30).unwrap();
    let feed = scratch.join("feed.jsonl");
    fs::write(&feed, text).unwrap();
    // A `.pub` file, and a key line alone.
    let other_pub = scratch.join("other.pub");
    fs::write(&other_pub, other.public.to_box().unwrap().to_bytes()).unwrap();
    let signer_line = scratch.join("signer.pub");
    fs::write(&signer_line, signer.public.to_base64()).unwrap();

    verify(&feed, &[other_pub.clone(), signer_line.clone()], 0).unwrap();
    verify(&feed, std::slice::from_ref(&signer_line), 29).unwrap();
    let refused = verify(&feed, &[other_pub], 0).unwrap_err();
    assert!(refused.ends_with("no key the app has made it"), "{refused}");
    let refused = verify(&feed, &[signer_line], 31).unwrap_err();
    assert!(
        refused.ends_with("fewer than 31: sign it again"),
        "{refused}"
    );
}
