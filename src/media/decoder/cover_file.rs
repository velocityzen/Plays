//! Cover art kept beside a sound file, as music folders often have it:
//! `cover.jpg`, `.jpeg`, `.png`, `.tif` or `.tiff`, named in any case. It
//! shows for a file with no picture of its own: neither cover art in its
//! tags nor video. It is looked for, and decoded, on a thread of its own,
//! as a picture can be large and the folder on a share far away.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use anyhow::{Context as _, Result, anyhow};
use ffmpeg::{Packet, media};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;

use super::video::VideoDecoder;
use super::{Shared, Source, StreamInfo, open_input};
use crate::media::source::is_file;

/// What a cover file is called, before its extension.
const NAME: &str = "cover";

/// The extensions a cover file can have, the one picked first first.
const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff"];

/// Look for the cover beside `source`, a file, and show it as `shared`'s,
/// scaled down to `max_short_side`.
pub(super) fn show(source: &Source, shared: &Arc<Shared>, max_short_side: Option<u32>) {
    let Source::File(file) = source else {
        return;
    };
    let beside = file.clone();
    let shared = shared.clone();
    let _ = thread::Builder::new()
        .name("cover file".into())
        .spawn(move || {
            let Some(path) = find(&beside) else {
                return;
            };
            match open(&path, max_short_side) {
                Ok(cover) => shared.set_cover(cover),
                Err(err) => log::warn!("{}: not showing it as the cover: {err:#}", path.display()),
            }
        })
        .inspect_err(|err| log::warn!("{}: not looking for a cover: {err}", file.display()));
}

/// The cover file beside `file`, if any: of those, the one whose extension
/// comes first in `EXTENSIONS`.
pub(super) fn find(file: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(file.parent()?).ok()?;
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let (stem, extension) = name.to_str()?.rsplit_once('.')?;
            let rank = EXTENSIONS
                .iter()
                .position(|known| extension.eq_ignore_ascii_case(known))?;
            stem.eq_ignore_ascii_case(NAME).then_some((rank, entry))
        })
        .filter(|(_, entry)| is_file(entry))
        .map(|(rank, entry)| (rank, entry.path()))
        .min()
        .map(|(_, path)| path)
}

/// The picture in the file at `path`, decoded as cover art in a file's
/// tags is.
fn open(path: &Path, max_short_side: Option<u32>) -> Result<Arc<RenderImage>> {
    let mut input = open_input(&Source::File(path.to_owned()), || false)?;
    let info = input
        .streams()
        .best(media::Type::Video)
        .map(|stream| StreamInfo::of(&stream))
        .ok_or_else(|| anyhow!("it isn't a picture"))?;
    let mut decoder = VideoDecoder::new(&info, max_short_side, false)?;
    let mut packet = Packet::empty();
    loop {
        packet.read(&mut input).context("reading it")?;
        if packet.stream() == info.index {
            return decoder
                .decode_still(&packet)
                .ok_or_else(|| anyhow!("its picture can't be decoded"));
        }
    }
}

#[cfg(test)]
#[path = "tests/cover_file.rs"]
mod tests;
