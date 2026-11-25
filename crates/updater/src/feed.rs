//! The feed: the app's releases, newest first, as JSON Lines, read as it
//! comes and no further than the running version.
//!
//! ```text
//! {"signature":"untrusted comment: …\nRU…\ntrusted comment: …\n…"}
//! {"app":"plays","latest":"0.3.0","published":…,"expires":…,"first":"<sha256>"}
//! {"version":"0.3.0","published":…,"notes":"…","downloads":{…},"next":"<sha256>"}
//! {"version":"0.2.0","published":…,"notes":"…","downloads":{…},"next":null}
//! ```
//!
//! - The first line is minisign's signature of the second, the header,
//!   which names the app, the latest version, when the feed expires, and
//!   the SHA-256 of the release after it.
//! - Each release names the SHA-256 of the line after it, `null` for the
//!   oldest, so the one signature covers them all, and none can be left
//!   out, put in or reordered.
//! - Versions go down, the first the header's latest. Reading stops at
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
use serde::{Deserialize, Serialize};
use url::Url;

use crate::hash;
use crate::{Error, Result};

/// The most the signature's and the header's lines may be, in bytes.
const HEAD_LIMIT: u64 = 16 << 10;
/// The most a release's line may be.
const RELEASE_LIMIT: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SignatureLine {
    signature: String,
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
    /// The SHA-256 of the line after it, the latest release's.
    pub first: String,
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

/// A release's line: it, and the SHA-256 of the line after it.
#[derive(Serialize, Deserialize)]
struct ReleaseLine {
    #[serde(flatten)]
    release: Release,
    next: Option<String>,
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

    /// Whether `signature`, as minisign writes it, holds for `bytes` by
    /// any of these keys.
    pub fn verify(&self, bytes: &[u8], signature: &str) -> Result<()> {
        let signature = Signature::decode(signature)
            .map_err(|_| Error::Signature("it isn't a minisign signature".into()))?;
        let mut why = None;
        for key in &self.0 {
            match key.verify(bytes, &signature, false) {
                Ok(()) => return Ok(()),
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

/// Read the feed from `lines` as it comes, if it's `app`'s, signed by one
/// of `keys`, and hasn't expired at `now`: the releases newer than
/// `running`, reading no further; `None` if there are none.
pub fn read(
    mut lines: impl BufRead,
    keys: &Keys,
    app: &str,
    running: &Version,
    now: u64,
) -> Result<Option<Feed>> {
    let signature: SignatureLine = parse(
        &line(&mut lines, HEAD_LIMIT)?.ok_or_else(empty)?,
        "its signature",
    )?;
    let header_line = line(&mut lines, HEAD_LIMIT)?.ok_or_else(empty)?;
    keys.verify(&header_line, &signature.signature)?;
    let header: Header = parse(&header_line, "its header")?;
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
    let mut expected = Some(header.first.clone());
    while let Some(sha256) = expected {
        let said = releases.last().map_or_else(
            || "the latest release".to_owned(),
            |release| format!("the release before {}", release.version),
        );
        let line = line(&mut lines, RELEASE_LIMIT)?
            .ok_or_else(|| Error::Feed(format!("it ends before {said}")))?;
        if !hash::same(&hash::of(&line), &sha256) {
            return Err(Error::Feed(format!("{said} isn't the one it names")));
        }
        let ReleaseLine { release, next } = parse(&line, &said)?;
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
        expected = next;
    }
    Ok(Some(Feed { header, releases }))
}

fn empty() -> Error {
    Error::Feed("it's empty".into())
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

fn parse<T: for<'de> Deserialize<'de>>(line: &[u8], what: &str) -> Result<T> {
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

/// A release's line, naming the SHA-256 of `next`, the line after it, if
/// there is one.
pub fn release_line(release: &Release, next: Option<&str>) -> String {
    let line = ReleaseLine {
        release: release.clone(),
        next: next.map(|next| hash::of(next.as_bytes())),
    };
    serde_json::to_string(&line).expect("a release is JSON")
}

/// The header's line, for `first`, the latest release's line.
pub fn header_line(app: &str, published: u64, expires: u64, first: &str) -> Result<String> {
    let release: ReleaseLine = parse(first.as_bytes(), "the latest release")?;
    let header = Header {
        app: app.to_owned(),
        latest: release.release.version,
        published,
        expires,
        first: hash::of(first.as_bytes()),
    };
    Ok(serde_json::to_string(&header).expect("a header is JSON"))
}

/// The feed: `signature`, minisign's of `header`, then the header and
/// the releases' lines.
pub fn join(signature: &str, header: &str, releases: &[String]) -> String {
    let signature = SignatureLine {
        signature: signature.to_owned(),
    };
    let mut feed = serde_json::to_string(&signature).expect("a signature is JSON");
    for line in std::iter::once(header).chain(releases.iter().map(String::as_str)) {
        feed.push('\n');
        feed.push_str(line);
    }
    feed.push('\n');
    feed
}

/// The releases' lines in the feed `text`, as they are, so a release can
/// go before them without changing them.
pub fn release_lines(text: &str) -> Vec<String> {
    text.lines()
        .skip(2)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Each release in the lines of `releases`, as `release_lines` gives them.
pub fn releases(releases: &[String]) -> Result<Vec<Release>> {
    releases
        .iter()
        .map(|line| parse::<ReleaseLine>(line.as_bytes(), "a release").map(|line| line.release))
        .collect()
}

#[cfg(test)]
#[path = "tests/feed.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "tests/feed_properties.rs"]
mod properties;
