//! The feed: the app's releases, newest first, as JSON Lines, read as it
//! comes and no further than the running version.
//!
//! ```text
//! {"signature":"untrusted comment: …\nRU…\ntrusted comment: plays\n…","header":{"app":"plays","latest":"0.3.0","published":…,"expires":…}}
//! {"signature":"…","release":{"version":"0.3.0","published":…,"notes":"…","downloads":{…}}}
//! {"signature":"…","release":{"version":"0.2.0",…}}
//! ```
//!
//! - Each line is a record, the header or a release, and minisign's
//!   signature of the record's JSON as it is in the line. Each signature's
//!   trusted comment is the app, so another app's line, signed with the
//!   same key, isn't taken for this one's.
//! - The header names the app, the latest version, and when the feed
//!   expires. Signing it again keeps the releases' lines as they are.
//! - Versions go down, the first the header's latest, so none can be put
//!   in or reordered. One left out hides only its notes. Reading stops at
//!   the header if the latest is the running version or older, and at the
//!   first release that is.
//! - Notes are Markdown. A release's downloads, by platform, are the whole
//!   install and deltas to it from earlier versions, each with its size
//!   and SHA-256. A release may be for some platforms alone, as a fix for
//!   one OS is; each platform's newest is the one it updates to.

use std::collections::BTreeMap;
use std::io::{BufRead, Read};

use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use url::Url;

use crate::hash;
use crate::{Error, Result};

/// The most the header's line may be, in bytes.
const HEADER_LIMIT: u64 = 16 << 10;
/// The most a release's line may be.
const RELEASE_LIMIT: u64 = 1 << 20;

const HEADER: &str = "header";
const RELEASE: &str = "release";

/// A line: a record, the header or a release, and minisign's signature of
/// its JSON as it is in the line.
#[derive(Deserialize)]
struct Line<'a> {
    signature: String,
    #[serde(borrow)]
    header: Option<&'a RawValue>,
    #[serde(borrow)]
    release: Option<&'a RawValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// The app it's for, so one app's feed is never taken for another's.
    pub app: String,
    pub latest: Version,
    /// When it was published, and when it stops being taken, in seconds
    /// since 1970 (UTC): a feed served past its time could be one held
    /// back, keeping the app from a newer release.
    pub published: u64,
    pub expires: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub version: Version,
    /// In seconds since 1970.
    pub published: u64,
    /// What's new in it, in Markdown.
    #[serde(default)]
    pub notes: String,
    /// Each platform's, by `platform()`.
    #[serde(default)]
    pub downloads: BTreeMap<String, Download>,
}

/// A platform's update: the whole install, and deltas from earlier
/// versions to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Download {
    #[serde(flatten)]
    pub whole: File,
    /// By the version each updates from.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deltas: BTreeMap<String, File>,
}

/// A file to download, as it must come.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File {
    /// Absolute, or relative to the feed's own.
    pub url: String,
    pub size: u64,
    /// In lowercase hex.
    pub sha256: String,
}

impl File {
    /// Its URL, relative to the feed's at `base`.
    pub fn url(&self, base: &Url) -> Result<Url> {
        base.join(&self.url)
            .map_err(|err| Error::Feed(format!("{}: {err}", self.url)))
    }
}

/// What a feed says that's newer than the running version.
#[derive(Clone, Debug, PartialEq)]
pub struct Feed {
    pub header: Header,
    /// Each release newer than the running version, newest first.
    pub releases: Vec<Release>,
}

impl Feed {
    /// The latest release.
    pub fn latest(&self) -> &Release {
        &self.releases[0]
    }
}

/// The platform updates are picked by: the OS and the processor, as Rust
/// names them, such as `macos-aarch64` or `linux-x86_64`.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The public keys a feed may be signed with: more than one while a key
/// is being replaced.
#[derive(Clone)]
pub struct Keys(Vec<PublicKey>);

impl Keys {
    /// Each key as minisign writes it: a `.pub` file's text, or its key
    /// line alone (`RW…`).
    pub fn parse<'a>(keys: impl IntoIterator<Item = &'a str>) -> Result<Self> {
        let keys = keys
            .into_iter()
            .map(|key| {
                let key = key.trim();
                let parsed = if key.contains('\n') {
                    PublicKey::decode(key)
                } else {
                    PublicKey::from_base64(key)
                };
                parsed
                    .map_err(|err| Error::Signature(format!("a key the app has isn't one: {err}")))
            })
            .collect::<Result<Vec<_>>>()?;
        if keys.is_empty() {
            return Err(Error::Signature(
                "the app has no key to check it with".into(),
            ));
        }
        Ok(Self(keys))
    }

    /// The trusted comment of `signature`, as minisign writes it, once it
    /// holds for `bytes` by any of these keys.
    pub fn verify(&self, bytes: &[u8], signature: &str) -> Result<String> {
        let signature = Signature::decode(signature)
            .map_err(|_| Error::Signature("it isn't a minisign signature".into()))?;
        let mut why = None;
        for key in &self.0 {
            match key.verify(bytes, &signature, false) {
                Ok(()) => return Ok(signature.trusted_comment().to_owned()),
                Err(minisign_verify::Error::UnexpectedKeyId) => {}
                Err(err) => why = Some(err.to_string()),
            }
        }
        Err(Error::Signature(
            why.unwrap_or_else(|| "no key the app has made it".into()),
        ))
    }
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keys({})", self.0.len())
    }
}

/// Read the feed from `lines` as it comes, if it's `app`'s, each line
/// signed by one of `keys`, and hasn't expired at `now`: the releases newer
/// than `running`, reading no further; `None` if there are none.
pub fn read(
    mut lines: impl BufRead,
    keys: &Keys,
    app: &str,
    running: &Version,
    now: u64,
) -> Result<Option<Feed>> {
    let first = line(&mut lines, HEADER_LIMIT)?.ok_or_else(|| Error::Feed("it's empty".into()))?;
    let header: Header = signed(&first, HEADER, keys, app, "the header")?;
    if header.app != app {
        return Err(Error::Feed(format!("it's {}'s", header.app)));
    }
    if now > header.expires {
        return Err(Error::Expired);
    }
    if header.latest <= *running {
        return Ok(None);
    }
    let mut releases: Vec<Release> = Vec::new();
    loop {
        let said = releases.last().map_or_else(
            || "the latest release".to_owned(),
            |release| format!("the release before {}", release.version),
        );
        let Some(line) = line(&mut lines, RELEASE_LIMIT)? else {
            if releases.is_empty() {
                return Err(Error::Feed(format!("it ends before {said}")));
            }
            break;
        };
        let release: Release = signed(&line, RELEASE, keys, app, &said)?;
        let in_order = match releases.last() {
            None => release.version == header.latest,
            Some(newer) => release.version < newer.version,
        };
        if !in_order {
            return Err(Error::Feed(format!("{said} is {}", release.version)));
        }
        if release.version <= *running {
            break;
        }
        check_downloads(&release)?;
        releases.push(release);
    }
    Ok(Some(Feed { header, releases }))
}

/// The record `name` in `line`, `what` in the feed, once its signature
/// holds by one of `keys` and is `app`'s.
fn signed<T: DeserializeOwned>(
    line: &[u8],
    name: &str,
    keys: &Keys,
    app: &str,
    what: &str,
) -> Result<T> {
    let (signature, record) = record(line, name, what)?;
    let signed_for = keys
        .verify(record.as_bytes(), &signature)
        .map_err(|err| match err {
            Error::Signature(why) => Error::Signature(format!("{what}: {why}")),
            err => err,
        })?;
    if signed_for != app {
        return Err(Error::Feed(format!("{what} is signed for {signed_for}")));
    }
    parse(record.as_bytes(), what)
}

/// The signature in `line`, `what` in the feed, and the JSON of its record
/// `name`, as it is in the line.
fn record<'a>(line: &'a [u8], name: &str, what: &str) -> Result<(String, &'a str)> {
    let Line {
        signature,
        header,
        release,
    } = parse(line, what)?;
    let record = if name == HEADER { header } else { release };
    let record = record.ok_or_else(|| Error::Feed(format!("{what}: the line has no {name}")))?;
    Ok((signature, record.get()))
}

/// The next line, `\n` left off, if it's no more than `limit` bytes.
fn line(lines: &mut impl BufRead, limit: u64) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    lines
        .by_ref()
        .take(limit + 1)
        .read_until(b'\n', &mut line)?;
    if line.last() == Some(&b'\n') {
        line.pop();
    } else if line.len() as u64 > limit {
        return Err(Error::Feed(format!("a line is over {limit} bytes")));
    }
    Ok((!line.is_empty()).then_some(line))
}

fn parse<'a, T: Deserialize<'a>>(line: &'a [u8], what: &str) -> Result<T> {
    serde_json::from_slice(line).map_err(|err| Error::Feed(format!("{what}: {err}")))
}

/// That each of `release`'s downloads has a SHA-256 that is one.
fn check_downloads(release: &Release) -> Result<()> {
    let check = |what: &str, file: &File| {
        if hash::parse(&file.sha256).is_none() {
            return Err(Error::Feed(format!("{what}'s SHA-256 isn't one")));
        }
        Ok(())
    };
    for (platform, download) in &release.downloads {
        check(
            &format!("{} for {platform}", release.version),
            &download.whole,
        )?;
        for (from, delta) in &download.deltas {
            check(
                &format!("{} for {platform} from {from}", release.version),
                delta,
            )?;
        }
    }
    Ok(())
}

/// The header's line, signed by `sign`, which gives minisign's signature,
/// its trusted comment the app, of the text it's given.
pub fn header_line<E>(
    header: &Header,
    sign: impl FnOnce(&str) -> std::result::Result<String, E>,
) -> std::result::Result<String, E> {
    signed_line(
        HEADER,
        &serde_json::to_string(header).expect("a header is JSON"),
        sign,
    )
}

/// A release's line, signed by `sign`, as for `header_line`.
pub fn release_line<E>(
    release: &Release,
    sign: impl FnOnce(&str) -> std::result::Result<String, E>,
) -> std::result::Result<String, E> {
    signed_line(
        RELEASE,
        &serde_json::to_string(release).expect("a release is JSON"),
        sign,
    )
}

fn signed_line<E>(
    name: &str,
    record: &str,
    sign: impl FnOnce(&str) -> std::result::Result<String, E>,
) -> std::result::Result<String, E> {
    let signature = serde_json::to_string(&sign(record)?).expect("a signature is JSON");
    Ok(format!("{{\"signature\":{signature},\"{name}\":{record}}}"))
}

/// The feed: the header's line, then the releases'.
pub fn join(header: &str, releases: &[String]) -> String {
    let mut feed = String::new();
    for line in std::iter::once(header).chain(releases.iter().map(String::as_str)) {
        feed.push_str(line);
        feed.push('\n');
    }
    feed
}

/// The header of the feed `text`, its signature unchecked.
pub fn header(text: &str) -> Result<Header> {
    let first = text.lines().next().unwrap_or_default();
    let (_, header) = record(first.as_bytes(), HEADER, "the header")?;
    parse(header.as_bytes(), "the header")
}

/// The releases' lines in the feed `text`, as they are, so a release can
/// go before them, or the header be signed again, without changing them.
pub fn release_lines(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Each release in the lines of `releases`, as `release_lines` gives them,
/// their signatures unchecked.
pub fn releases(releases: &[String]) -> Result<Vec<Release>> {
    releases
        .iter()
        .map(|line| release_record(line).map(|(_, release)| release))
        .collect()
}

/// A release's `line`: its signature, unchecked, and the release.
pub fn release_record(line: &str) -> Result<(String, Release)> {
    let (signature, release) = record(line.as_bytes(), RELEASE, "a release")?;
    Ok((signature, parse(release.as_bytes(), "a release")?))
}

#[cfg(test)]
#[path = "tests/feed.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "tests/feed_properties.rs"]
mod properties;
