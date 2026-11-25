use std::collections::BTreeMap;
use std::io::{self, BufReader, Cursor, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use minisign::KeyPair;

use super::*;

pub(crate) const APP: &str = "plays";
/// When the feeds here are read: before any expires.
pub(crate) const NOW: u64 = 1_800_000_000;
const EXPIRES: u64 = NOW + 86_400;

pub(crate) fn key() -> KeyPair {
    KeyPair::generate_unencrypted_keypair().expect("a key")
}

pub(crate) fn keys(key: &KeyPair) -> Keys {
    Keys::parse([key.pk.to_base64().as_str()]).expect("the key")
}

pub(crate) fn version(text: &str) -> Version {
    Version::parse(text).expect("a version")
}

/// A release of `version`, with a download for `macos-aarch64`.
pub(crate) fn release(version: &str) -> Release {
    Release {
        version: self::version(version),
        published: NOW - 3_600,
        notes: format!("## {version}\n\n- Plays **plays** more."),
        downloads: BTreeMap::from([(
            "macos-aarch64".to_owned(),
            Download {
                whole: File {
                    url: format!("Plays-{version}.update"),
                    size: 1_000,
                    sha256: hash::of(version.as_bytes()),
                },
                deltas: BTreeMap::new(),
            },
        )]),
    }
}

/// The releases' lines, each naming the one after it.
pub(crate) fn lines_of(releases: &[Release]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for release in releases.iter().rev() {
        let line = release_line(release, lines.first().map(String::as_str));
        lines.insert(0, line);
    }
    lines
}

/// The feed of `releases`, newest first, signed with `key`, expiring at
/// `expires`.
pub(crate) fn feed_expiring(key: &KeyPair, releases: &[Release], expires: u64) -> String {
    let lines = lines_of(releases);
    let header = header_line(APP, NOW - 60, expires, &lines[0]).expect("a header");
    join(&sign(key, &header), &header, &lines)
}

pub(crate) fn feed(key: &KeyPair, releases: &[Release]) -> String {
    feed_expiring(key, releases, EXPIRES)
}

pub(crate) fn sign(key: &KeyPair, text: &str) -> String {
    minisign::sign(Some(&key.pk), &key.sk, Cursor::new(text), None, None)
        .expect("signed")
        .into_string()
}

fn read_text(text: &str, keys: &Keys, running: &str) -> Result<Option<Feed>> {
    read(Cursor::new(text), keys, APP, &version(running), NOW)
}

fn versions(feed: &Feed) -> Vec<String> {
    feed.releases
        .iter()
        .map(|release| release.version.to_string())
        .collect()
}

/// A reader of `text`, then of whatever, counting each read past `text`.
fn reading(text: &str, past: Arc<AtomicUsize>) -> impl io::BufRead {
    struct Past(Arc<AtomicUsize>);
    impl Read for Past {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.fetch_add(1, Ordering::Relaxed);
            buf.fill(b'x');
            Ok(buf.len())
        }
    }
    BufReader::with_capacity(16, Cursor::new(text.to_owned()).chain(Past(past)))
}

#[test]
fn has_the_releases_newer_than_the_running_one_newest_first() {
    let key = key();
    let text = feed(
        &key,
        &[
            release("0.4.0"),
            release("0.3.1"),
            release("0.3.0"),
            release("0.2.0"),
        ],
    );
    let feed = read_text(&text, &keys(&key), "0.3.0").unwrap().unwrap();
    assert_eq!(versions(&feed), ["0.4.0", "0.3.1"]);
    assert_eq!(feed.header.latest, version("0.4.0"));
    assert_eq!(feed.latest().notes, "## 0.4.0\n\n- Plays **plays** more.");
}

#[test]
fn has_them_all_for_a_version_older_than_any() {
    let key = key();
    let text = feed(&key, &[release("0.3.0"), release("0.2.0")]);
    let feed = read_text(&text, &keys(&key), "0.1.0").unwrap().unwrap();
    assert_eq!(versions(&feed), ["0.3.0", "0.2.0"]);
}

#[test]
fn is_none_when_the_running_version_is_the_latest_or_newer() {
    let key = key();
    let text = feed(&key, &[release("0.3.0"), release("0.2.0")]);
    assert_eq!(read_text(&text, &keys(&key), "0.3.0").unwrap(), None);
    assert_eq!(read_text(&text, &keys(&key), "0.4.0-beta.1").unwrap(), None);
    // A prerelease of the latest is older than it.
    assert!(
        read_text(&text, &keys(&key), "0.3.0-rc.1")
            .unwrap()
            .is_some()
    );
}

#[test]
fn reads_no_further_than_the_header_when_up_to_date() {
    let key = key();
    let lines = lines_of(&[release("0.3.0")]);
    let header = header_line(APP, NOW, EXPIRES, &lines[0]).unwrap();
    // The header, and nothing after it but what isn't a feed.
    let text = format!(
        "{}\n{header}\n",
        serde_json::json!({"signature": sign(&key, &header)})
    );
    let past = Arc::new(AtomicUsize::new(0));
    let read = read(
        reading(&text, past.clone()),
        &keys(&key),
        APP,
        &version("0.3.0"),
        NOW,
    );
    assert_eq!(read.unwrap(), None);
    assert_eq!(past.load(Ordering::Relaxed), 0);
}

#[test]
fn reads_no_further_than_the_running_version() {
    let key = key();
    let text = feed(
        &key,
        &[release("0.3.0"), release("0.2.0"), release("0.1.0")],
    );
    // Up to 0.2.0's line, then nothing that's the feed.
    let cut = text.lines().take(4).collect::<Vec<_>>().join("\n") + "\n";
    let past = Arc::new(AtomicUsize::new(0));
    let read = read(
        reading(&cut, past.clone()),
        &keys(&key),
        APP,
        &version("0.2.0"),
        NOW,
    );
    assert_eq!(versions(&read.unwrap().unwrap()), ["0.3.0"]);
    assert_eq!(past.load(Ordering::Relaxed), 0);
}

#[test]
fn is_taken_signed_by_any_key_the_app_has() {
    let (old, new) = (key(), key());
    let both = Keys::parse([old.pk.to_base64().as_str(), new.pk.to_base64().as_str()]).unwrap();
    let text = feed(&new, &[release("0.2.0")]);
    assert!(read_text(&text, &both, "0.1.0").unwrap().is_some());
    // As minisign writes a .pub file, too.
    let file = new.pk.to_box().unwrap().into_string();
    assert!(
        read_text(&text, &Keys::parse([file.as_str()]).unwrap(), "0.1.0")
            .unwrap()
            .is_some()
    );
}

#[test]
fn isnt_taken_signed_by_another_key() {
    let (key, other) = (self::key(), self::key());
    let text = feed(&other, &[release("0.2.0")]);
    assert!(matches!(
        read_text(&text, &keys(&key), "0.1.0"),
        Err(Error::Signature(_))
    ));
}

#[test]
fn isnt_taken_with_its_header_changed() {
    let key = key();
    let text = feed(&key, &[release("0.2.0")]);
    let changed = text.replacen("\"app\":\"plays\"", "\"app\":\"plays\" ", 1);
    assert!(matches!(
        read_text(&changed, &keys(&key), "0.1.0"),
        Err(Error::Signature(_))
    ));
}

#[test]
fn isnt_taken_for_another_app() {
    let key = key();
    let lines = lines_of(&[release("0.2.0")]);
    let header = header_line("other", NOW, EXPIRES, &lines[0]).unwrap();
    let text = join(&sign(&key, &header), &header, &lines);
    let read = read_text(&text, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why == "it's other's"));
}

#[test]
fn isnt_taken_once_it_has_expired() {
    let key = key();
    let text = feed_expiring(&key, &[release("0.2.0")], NOW - 1);
    assert!(matches!(
        read_text(&text, &keys(&key), "0.1.0"),
        Err(Error::Expired)
    ));
}

#[test]
fn isnt_taken_with_a_release_changed() {
    let key = key();
    let text = feed(&key, &[release("0.3.0"), release("0.2.0")]);
    let changed = text.replacen("Plays-0.2.0.update", "Plays-0.2.1.update", 1);
    let read = read_text(&changed, &keys(&key), "0.1.0");
    assert!(
        matches!(read, Err(Error::Feed(why)) if why == "the release before 0.3.0 isn't the one it names")
    );
}

#[test]
fn isnt_taken_with_a_release_left_out_or_put_in() {
    let key = key();
    let text = feed(
        &key,
        &[release("0.3.0"), release("0.2.0"), release("0.1.0")],
    );
    let mut lines: Vec<&str> = text.lines().collect();
    lines.remove(3);
    let left_out = lines.join("\n");
    assert!(matches!(
        read_text(&left_out, &keys(&key), "0.0.1"),
        Err(Error::Feed(_))
    ));

    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    lines.insert(3, release_line(&release("0.2.5"), Some(&lines[3])));
    assert!(matches!(
        read_text(&lines.join("\n"), &keys(&key), "0.0.1"),
        Err(Error::Feed(_))
    ));
}

#[test]
fn isnt_taken_cut_short() {
    let key = key();
    let text = feed(&key, &[release("0.3.0"), release("0.2.0")]);
    let cut = text.lines().take(3).collect::<Vec<_>>().join("\n");
    let read = read_text(&cut, &keys(&key), "0.1.0");
    assert!(
        matches!(read, Err(Error::Feed(why)) if why == "it ends before the release before 0.3.0")
    );
}

#[test]
fn isnt_taken_out_of_order() {
    let key = key();
    // Each line names the next, but 0.2.0 comes before 0.3.0.
    let text = feed(
        &key,
        &[release("0.4.0"), release("0.2.0"), release("0.3.0")],
    );
    let read = read_text(&text, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why == "the release before 0.2.0 is 0.3.0"));
}

#[test]
fn isnt_taken_when_the_latest_isnt_the_headers() {
    let key = key();
    let lines = lines_of(&[release("0.2.0")]);
    let mut header: Header =
        serde_json::from_str(&header_line(APP, NOW, EXPIRES, &lines[0]).unwrap()).unwrap();
    header.latest = version("0.3.0");
    let header = serde_json::to_string(&header).unwrap();
    let text = join(&sign(&key, &header), &header, &lines);
    assert!(matches!(
        read_text(&text, &keys(&key), "0.1.0"),
        Err(Error::Feed(_))
    ));
}

#[test]
fn isnt_taken_with_a_line_too_long() {
    let key = key();
    let mut long = release("0.2.0");
    long.notes = "x".repeat(RELEASE_LIMIT as usize);
    let text = feed(&key, &[long]);
    let read = read_text(&text, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why.starts_with("a line is over")));
}

#[test]
fn isnt_taken_with_a_sha256_that_isnt_one() {
    let key = key();
    let mut bad = release("0.2.0");
    bad.downloads.get_mut("macos-aarch64").unwrap().whole.sha256 = "abc".into();
    let read = read_text(&feed(&key, &[bad]), &keys(&key), "0.1.0");
    assert!(
        matches!(read, Err(Error::Feed(why)) if why == "0.2.0 for macos-aarch64's SHA-256 isn't one")
    );
}

#[test]
fn isnt_taken_empty_or_unsigned() {
    let key = key();
    assert!(matches!(
        read_text("", &keys(&key), "0.1.0"),
        Err(Error::Feed(_))
    ));
    let text = feed(&key, &[release("0.2.0")]);
    let unsigned = text.lines().skip(1).collect::<Vec<_>>().join("\n");
    assert!(read_text(&unsigned, &keys(&key), "0.1.0").is_err());
}

#[test]
fn a_release_goes_before_the_others_leaving_their_lines_as_they_are() {
    let key = key();
    let text = feed(&key, &[release("0.2.0"), release("0.1.0")]);
    let old = release_lines(&text);
    let mut lines = vec![release_line(&release("0.3.0"), Some(&old[0]))];
    lines.extend(old.iter().cloned());
    let header = header_line(APP, NOW, EXPIRES, &lines[0]).unwrap();
    let text = join(&sign(&key, &header), &header, &lines);
    let feed = read_text(&text, &keys(&key), "0.0.1").unwrap().unwrap();
    assert_eq!(versions(&feed), ["0.3.0", "0.2.0", "0.1.0"]);
    assert_eq!(releases(&lines).unwrap(), feed.releases);
}

#[test]
fn a_files_url_is_relative_to_the_feeds() {
    let feed = Url::parse("https://example.com/plays/feed.jsonl").unwrap();
    let file = |url: &str| File {
        url: url.into(),
        size: 0,
        sha256: String::new(),
    };
    assert_eq!(
        file("Plays.update").url(&feed).unwrap().as_str(),
        "https://example.com/plays/Plays.update"
    );
    assert_eq!(
        file("https://cdn.example.com/a")
            .url(&feed)
            .unwrap()
            .as_str(),
        "https://cdn.example.com/a"
    );
}

#[test]
fn a_key_that_isnt_one_or_none_is_refused() {
    assert!(matches!(Keys::parse(["RWnope"]), Err(Error::Signature(_))));
    assert!(matches!(Keys::parse([]), Err(Error::Signature(_))));
}

#[test]
fn the_platform_is_the_os_and_processor() {
    let platform = platform();
    assert_eq!(
        platform,
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    );
    assert!(!platform.contains(' '));
}
