use std::collections::BTreeMap;
use std::convert::Infallible;
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

/// minisign's signature of `text` with `key`, its trusted comment `app`.
pub(crate) fn sign_for(app: &str, key: &KeyPair, text: &str) -> String {
    minisign::sign(Some(&key.pk), &key.sk, Cursor::new(text), Some(app), None)
        .expect("signed")
        .into_string()
}

/// `release`'s line, signed with `key` for `app`.
pub(crate) fn release_signed_for(app: &str, key: &KeyPair, release: &Release) -> String {
    release_line(release, |text| {
        Ok::<_, Infallible>(sign_for(app, key, text))
    })
    .unwrap()
}

/// The releases' lines, each signed with `key`.
pub(crate) fn lines_of(key: &KeyPair, releases: &[Release]) -> Vec<String> {
    releases
        .iter()
        .map(|release| release_signed_for(APP, key, release))
        .collect()
}

/// `app`'s header, `latest` its latest, expiring at `expires`.
pub(crate) fn header_of(app: &str, latest: &str, expires: u64) -> Header {
    Header {
        app: app.to_owned(),
        latest: version(latest),
        published: NOW - 60,
        expires,
    }
}

/// `header`'s line, signed with `key` for `app`.
pub(crate) fn header_signed_for(app: &str, key: &KeyPair, header: &Header) -> String {
    header_line(header, |text| Ok::<_, Infallible>(sign_for(app, key, text))).unwrap()
}

/// The feed of `releases`, newest first, signed with `key`, expiring at
/// `expires`.
pub(crate) fn feed_expiring(key: &KeyPair, releases: &[Release], expires: u64) -> String {
    let header = header_of(APP, &releases[0].version.to_string(), expires);
    join(
        &header_signed_for(APP, key, &header),
        &lines_of(key, releases),
    )
}

pub(crate) fn feed(key: &KeyPair, releases: &[Release]) -> String {
    feed_expiring(key, releases, EXPIRES)
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
    // The header, and nothing after it but what isn't a feed.
    let text = join(
        &header_signed_for(APP, &key, &header_of(APP, "0.3.0", EXPIRES)),
        &[],
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
    let cut = text.lines().take(3).collect::<Vec<_>>().join("\n") + "\n";
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
    let lines = lines_of(&key, &[release("0.2.0")]);
    let header = header_of("other", "0.2.0", EXPIRES);
    let text = join(&header_signed_for("other", &key, &header), &lines);
    let read = read_text(&text, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why == "the header is signed for other"));

    let text = join(&header_signed_for(APP, &key, &header), &lines);
    let read = read_text(&text, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why == "it's other's"));
}

#[test]
fn isnt_taken_with_another_apps_release_signed_with_the_same_key() {
    let key = key();
    let mut lines = lines_of(&key, &[release("0.3.0"), release("0.2.0")]);
    lines[1] = release_signed_for("other", &key, &release("0.2.0"));
    let header = header_signed_for(APP, &key, &header_of(APP, "0.3.0", EXPIRES));
    let read = read_text(&join(&header, &lines), &keys(&key), "0.1.0");
    assert!(
        matches!(read, Err(Error::Feed(why)) if why == "the release before 0.3.0 is signed for other")
    );
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
        matches!(read, Err(Error::Signature(why)) if why.starts_with("the release before 0.3.0: "))
    );
}

#[test]
fn isnt_taken_with_a_release_put_in_unsigned() {
    let (key, other) = (self::key(), self::key());
    let text = feed(&key, &[release("0.3.0"), release("0.1.0")]);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    lines.insert(2, release_signed_for(APP, &other, &release("0.2.0")));
    let read = read_text(&lines.join("\n"), &keys(&key), "0.0.1");
    assert!(
        matches!(read, Err(Error::Signature(why)) if why.starts_with("the release before 0.3.0: "))
    );
}

#[test]
fn a_release_left_out_is_read_without() {
    let key = key();
    let text = feed(
        &key,
        &[release("0.3.0"), release("0.2.0"), release("0.1.0")],
    );
    let mut lines: Vec<&str> = text.lines().collect();
    lines.remove(2);
    let feed = read_text(&lines.join("\n"), &keys(&key), "0.0.1")
        .unwrap()
        .unwrap();
    assert_eq!(versions(&feed), ["0.3.0", "0.1.0"]);
}

#[test]
fn isnt_taken_with_no_release_or_one_cut_short() {
    let key = key();
    let text = feed(&key, &[release("0.3.0"), release("0.2.0")]);
    let header = text.lines().next().unwrap();
    let read = read_text(header, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why == "it ends before the latest release"));

    let cut = &text[..text.rfind("\"}}").unwrap()];
    let read = read_text(cut, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why.starts_with("the release before 0.3.0: ")));
}

#[test]
fn isnt_taken_out_of_order() {
    let key = key();
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
    let header = header_signed_for(APP, &key, &header_of(APP, "0.3.0", EXPIRES));
    let text = join(&header, &lines_of(&key, &[release("0.2.0")]));
    let read = read_text(&text, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why == "the latest release is 0.2.0"));
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
    let read = read_text("", &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why == "it's empty"));

    let header = serde_json::to_string(&header_of(APP, "0.2.0", EXPIRES)).unwrap();
    let release = lines_of(&key, &[release("0.2.0")]).remove(0);
    let unsigned = format!("{{\"signature\":\"\",\"header\":{header}}}\n{release}\n");
    let read = read_text(&unsigned, &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Signature(why)) if why.starts_with("the header: ")));

    let bare = format!("{header}\n{release}\n");
    assert!(matches!(
        read_text(&bare, &keys(&key), "0.1.0"),
        Err(Error::Feed(_))
    ));
}

#[test]
fn isnt_taken_with_a_release_where_the_header_goes() {
    let key = key();
    let lines = lines_of(&key, &[release("0.2.0")]);
    let read = read_text(&join(&lines[0], &lines), &keys(&key), "0.1.0");
    assert!(matches!(read, Err(Error::Feed(why)) if why == "the header: the line has no header"));
}

#[test]
fn a_release_goes_before_the_others_leaving_their_lines_as_they_are() {
    let key = key();
    let text = feed(&key, &[release("0.2.0"), release("0.1.0")]);
    let old = release_lines(&text);
    let mut lines = lines_of(&key, &[release("0.3.0")]);
    lines.extend(old.iter().cloned());
    let header = header_signed_for(APP, &key, &header_of(APP, "0.3.0", EXPIRES));
    let text = join(&header, &lines);
    let feed = read_text(&text, &keys(&key), "0.0.1").unwrap().unwrap();
    assert_eq!(versions(&feed), ["0.3.0", "0.2.0", "0.1.0"]);
    assert_eq!(release_lines(&text)[1..], old[..]);
    assert_eq!(releases(&lines).unwrap(), feed.releases);
    assert_eq!(self::header(&text).unwrap(), feed.header);
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
