//! An HLS master playlist cut down to the variant that plays.
//!
//! Handed a master playlist, FFmpeg opens every variant it lists, one after
//! another, a segment or three of each, before the first frame; and as it
//! plays it reads each one whose streams aren't discarded. One offering a
//! dozen sizes of the same picture takes most of a minute to open, and then
//! wants the bandwidth of all of them. So, as a browser does, the variant is
//! picked first: the best within the resolution limit. FFmpeg opens a
//! playlist listing it alone, with the sound and subtitles it plays with.

use std::str;

use anyhow::{Context as _, Result};
use url::Url;

use super::fetch::fetch;
use crate::media::source::Source;

/// A master playlist longer than this isn't cut down.
const MAX_PLAYLIST_BYTES: usize = 1 << 20;

/// How a playlist's CODECS names each sound codec, before the first dot.
const SOUND_CODECS: [&str; 9] = [
    "mp4a", "ac-3", "ec-3", "ac-4", "opus", "flac", "alac", "mha1", "mhm1",
];

/// What FFmpeg opens for `source` when it is an HLS master playlist listing
/// more than one variant: a `data:` URL holding it cut down to the one to
/// play within `max_short_side`. `None` for anything else, to be opened as
/// it is. Reads the playlist to see, giving up once `stop` says so.
pub(super) fn variant(
    source: &Source,
    max_short_side: Option<u32>,
    stop: impl Fn() -> bool,
) -> Result<Option<String>> {
    let Source::Url(location) = source else {
        return Ok(None);
    };
    let Some(url) = Url::parse(location).ok().filter(is_playlist) else {
        return Ok(None);
    };
    let fetched = fetch(
        location,
        ffmpeg_safe::network_options(),
        MAX_PLAYLIST_BYTES,
        stop,
    )
    .with_context(|| format!("could not open {source}"))?;
    let Some(fetched) = fetched else {
        return Ok(None);
    };
    // Its addresses are relative to where it came from, after redirects.
    let base = Url::parse(&fetched.location).unwrap_or(url);
    let Ok(text) = str::from_utf8(&fetched.body) else {
        return Ok(None);
    };
    Ok(one_variant(text, &base, max_short_side).map(|cut| data_url(&cut)))
}

/// Whether `url` names an HLS playlist, over HTTP.
fn is_playlist(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    matches!(url.scheme(), "http" | "https") && (path.ends_with(".m3u8") || path.ends_with(".m3u"))
}

/// `playlist` as a `data:` URL. FFmpeg takes a playlist for HLS only when
/// its name or type says so, and a `data:` URL has neither: a comment, last,
/// names it.
fn data_url(playlist: &str) -> String {
    format!("data:application/vnd.apple.mpegurl,{playlist}# .m3u8")
}

/// The master playlist `text`, which came from `base`, cut down to the
/// variant to play and the renditions it plays with, every address in it
/// made whole. `None` unless it lists more than one variant, or when it
/// can't be cut as it is: it defines variables, a variant names no playlist,
/// or one kept is anywhere but HTTP.
pub(super) fn one_variant(text: &str, base: &Url, max_short_side: Option<u32>) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.first()?.trim_start_matches('\u{feff}').trim_end();
    if first != "#EXTM3U" || lines.iter().any(|l| l.starts_with("#EXT-X-DEFINE")) {
        return None;
    }
    let variants = variants(&lines)?;
    if variants.len() < 2 {
        return None;
    }
    let chosen = choose(&variants, max_short_side)?;
    let mut cut = String::new();
    for (at, line) in lines.iter().enumerate() {
        let kept = if at == chosen.at {
            line.to_string()
        } else if at == chosen.uri_at {
            whole(base, chosen.uri)?
        } else if variants.iter().any(|v| v.at == at || v.uri_at == at)
            || line.starts_with("#EXT-X-I-FRAME-STREAM-INF:")
        {
            continue;
        } else if let Some(list) = line.strip_prefix("#EXT-X-MEDIA:") {
            if !chosen.plays_with(list) {
                continue;
            }
            rendition(list, base)?
        } else {
            line.to_string()
        };
        cut.push_str(&kept);
        cut.push('\n');
    }
    Some(cut)
}

/// A variant a master playlist lists.
#[derive(Debug)]
struct Variant<'a> {
    /// The line of its `#EXT-X-STREAM-INF` tag.
    at: usize,
    /// The line naming its playlist, and what it says.
    uri_at: usize,
    uri: &'a str,
    attributes: Vec<(&'a str, &'a str)>,
    bandwidth: u64,
    /// The shorter side of its picture, when it says.
    short_side: Option<u32>,
    /// It has sound and no picture.
    sound_only: bool,
}

impl Variant<'_> {
    /// Whether the rendition whose `#EXT-X-MEDIA` attributes are `list` is in
    /// one of the groups this plays with. A variant names each group by the
    /// type of rendition in it: AUDIO, VIDEO, SUBTITLES, CLOSED-CAPTIONS.
    fn plays_with(&self, list: &str) -> bool {
        let rendition = attributes(list);
        let (Some(kind), Some(group)) = (
            attribute(&rendition, "TYPE"),
            attribute(&rendition, "GROUP-ID"),
        ) else {
            return false;
        };
        attribute(&self.attributes, kind) == Some(group)
    }
}

/// The variants in a master playlist's `lines`, each with the line naming
/// its playlist; `None` if one has none.
fn variants<'a>(lines: &[&'a str]) -> Option<Vec<Variant<'a>>> {
    let mut variants = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let Some(list) = line.strip_prefix("#EXT-X-STREAM-INF:") else {
            continue;
        };
        let (uri_at, uri) = lines
            .iter()
            .enumerate()
            .skip(at + 1)
            .map(|(at, line)| (at, line.trim()))
            .find(|(_, line)| !line.is_empty() && !line.starts_with('#'))?;
        let attributes = attributes(list);
        let short_side = attribute(&attributes, "RESOLUTION")
            .and_then(|size| size.split_once('x'))
            .and_then(|(width, height)| Some(width.parse::<u32>().ok()?.min(height.parse().ok()?)));
        let sound_only = short_side.is_none()
            && attribute(&attributes, "CODECS")
                .is_some_and(|codecs| codecs.split(',').all(is_sound));
        variants.push(Variant {
            at,
            uri_at,
            uri,
            bandwidth: attribute(&attributes, "BANDWIDTH")
                .and_then(|b| b.parse().ok())
                .unwrap_or(0),
            attributes,
            short_side,
            sound_only,
        });
    }
    Some(variants)
}

/// Whether `codec`, as a playlist's CODECS names it, is a sound codec.
fn is_sound(codec: &str) -> bool {
    let kind = codec.trim().split('.').next().unwrap_or_default();
    SOUND_CODECS
        .iter()
        .any(|sound| kind.eq_ignore_ascii_case(sound))
}

/// The variant to play: of those with a picture, if any, the one with the
/// most bandwidth whose picture fits within `max_short_side`, or of the
/// smallest if none does; the first listed of equals.
fn choose<'v, 'a>(
    variants: &'v [Variant<'a>],
    max_short_side: Option<u32>,
) -> Option<&'v Variant<'a>> {
    let pictures: Vec<&Variant> = variants.iter().filter(|v| !v.sound_only).collect();
    let pool = if pictures.is_empty() {
        variants.iter().collect()
    } else {
        pictures
    };
    let fits = |v: &&Variant| match (v.short_side, max_short_side) {
        (Some(side), Some(max)) => side <= max,
        _ => true,
    };
    let mut candidates: Vec<&Variant> = pool.iter().copied().filter(fits).collect();
    if candidates.is_empty() {
        let smallest = pool.iter().filter_map(|v| v.short_side).min();
        candidates = pool
            .into_iter()
            .filter(|v| v.short_side == smallest)
            .collect();
    }
    candidates.into_iter().reduce(|best, v| {
        if v.bandwidth > best.bandwidth {
            v
        } else {
            best
        }
    })
}

/// The `#EXT-X-MEDIA` tag with attributes `list`, its playlist's address
/// made whole; `None` if that's anywhere but HTTP.
fn rendition(list: &str, base: &Url) -> Option<String> {
    let mut written = Vec::new();
    for (name, value) in attributes(list) {
        if name == "URI" {
            written.push(format!("URI=\"{}\"", whole(base, unquoted(value))?));
        } else {
            written.push(format!("{name}={value}"));
        }
    }
    Some(format!("#EXT-X-MEDIA:{}", written.join(",")))
}

/// `uri`, relative to `base`, made whole; `None` if it's anywhere but HTTP.
fn whole(base: &Url, uri: &str) -> Option<String> {
    base.join(uri)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(String::from)
}

/// A tag's attributes, from its `list`: each `NAME=value` as written, a
/// quoted value with its quotes, which may hold commas.
fn attributes(list: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut rest = list;
    while let Some((name, after)) = rest.split_once('=') {
        let end = match after.strip_prefix('"') {
            Some(quoted) => quoted.find('"').map_or(after.len(), |close| close + 2),
            None => after.find(',').unwrap_or(after.len()),
        };
        found.push((name.trim(), &after[..end]));
        rest = after[end..].strip_prefix(',').unwrap_or(&after[end..]);
    }
    found
}

/// The value of attribute `name`, its quotes taken off.
fn attribute<'a>(attributes: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, value)| unquoted(value))
}

/// `value` without the quotes around it, if it has them.
fn unquoted(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

#[cfg(test)]
#[path = "tests/hls.rs"]
mod tests;
