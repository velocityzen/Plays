//! Where a file's HLS segments start. Copied video can only be cut at its
//! own keyframes, so segments start at the first keyframe at least the
//! target length after the last start, read from the file's index (or, with
//! none, by reading the file). Re-encoded video, and sound alone, are cut
//! every target length, re-encoded video with a keyframe made there.

use ffmpeg::ffi;
use ffmpeg::format::context::Input;
use ffmpeg_next as ffmpeg;

use crate::media::MediaInfo;
use crate::plan::{Plan, VideoPlan};
use crate::profile::Container;

/// The most segments a file is cut into, a week of six-second ones: a
/// broken or crafted file can claim to last for ever.
pub(crate) const MOST_SEGMENTS: usize = 100_000;
/// The longest a segment of copied video may be, in segment lengths:
/// video with keyframes further apart than this is re-encoded, since all
/// of a segment is held at once.
pub(crate) const LONGEST_COPIED: f64 = 5.0;

/// Which timestamp a keyframe is known by: MP4's index holds decoding
/// times, Matroska's (and a scan) presentation times.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Domain {
    Pts,
    Dts,
}

/// The segments of a file, in seconds of its timeline.
#[derive(Clone, Debug)]
pub(crate) struct Timeline {
    /// Each segment's start; the next one's start is its end.
    pub starts: Vec<f64>,
    /// Where the last segment ends.
    pub end: f64,
    /// For copied video: the keyframe each segment starts with, in the
    /// video stream's time base and `domain`, to seek to exactly.
    pub keyframes: Option<(Vec<i64>, Domain)>,
}

impl Timeline {
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn start(&self, segment: usize) -> f64 {
        self.starts[segment]
    }

    pub fn end_of(&self, segment: usize) -> f64 {
        self.starts.get(segment + 1).copied().unwrap_or(self.end)
    }

    pub fn duration(&self, segment: usize) -> f64 {
        (self.end_of(segment) - self.start(segment)).max(0.0)
    }

    /// How long the longest segment is.
    pub fn longest(&self) -> f64 {
        (0..self.len())
            .map(|segment| self.duration(segment))
            .fold(0.0_f64, f64::max)
    }

    /// The longest segment, rounded up, as HLS's target duration.
    pub fn target(&self) -> u64 {
        self.longest().max(1.0).ceil() as u64
    }
}

/// The segments for `plan`, each about `target` seconds long.
pub(crate) fn timeline(input: &mut Input, media: &MediaInfo, plan: &Plan, target: f64) -> Timeline {
    let start = media.start;
    let claimed = media.duration.unwrap_or(0.0);
    let end = start + claimed.min(MOST_SEGMENTS as f64 * target).max(0.0);
    if let Some(VideoPlan::Copy { stream, .. }) = plan.video {
        let time_base = f64::from(
            input
                .stream(stream)
                .map_or(ffmpeg::Rational::new(1, 1), |stream| stream.time_base()),
        );
        let (keyframes, domain) = match indexed_keyframes(input, stream) {
            keys if keys.len() > 1 => (
                keys,
                if media.container == Container::Mp4 {
                    Domain::Dts
                } else {
                    Domain::Pts
                },
            ),
            _ => (scanned_keyframes(input, stream), Domain::Pts),
        };
        if !keyframes.is_empty() {
            let mut starts = Vec::new();
            let mut kept = Vec::new();
            for key in keyframes {
                if starts.len() == MOST_SEGMENTS {
                    break;
                }
                let time = key as f64 * time_base;
                if starts
                    .last()
                    .is_none_or(|last: &f64| time - last >= target - 1e-3)
                {
                    starts.push(time);
                    kept.push(key);
                }
            }
            return Timeline {
                end: end.max(starts.last().copied().unwrap_or(start)),
                starts,
                keyframes: Some((kept, domain)),
            };
        }
    }
    let count = ((end - start) / target).ceil().max(1.0) as usize;
    Timeline {
        starts: (0..count).map(|n| start + n as f64 * target).collect(),
        end,
        keyframes: None,
    }
}

/// The keyframes the container's index lists for `stream`. Matroska reads
/// its index on the first seek, so seek first.
fn indexed_keyframes(input: &mut Input, stream: usize) -> Vec<i64> {
    let _ = input.seek(0, ..);
    let mut keys = Vec::new();
    // SAFETY: the stream exists; index entries are read while the context
    // is open and not changed meanwhile.
    unsafe {
        let st = *(*input.as_ptr()).streams.add(stream);
        let count = ffi::avformat_index_get_entries_count(st);
        for index in 0..count {
            let entry = ffi::avformat_index_get_entry(st, index);
            if !entry.is_null() && (*entry).flags() & ffi::AVINDEX_KEYFRAME != 0 {
                keys.push((*entry).timestamp);
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// The keyframes of `stream`, found by reading its packets.
fn scanned_keyframes(input: &mut Input, stream: usize) -> Vec<i64> {
    let _ = input.seek(0, ..);
    set_discard(input, Some(stream));
    let mut keys = Vec::new();
    for (packet_stream, packet) in input.packets() {
        if packet_stream.index() == stream
            && packet.is_key()
            && let Some(time) = packet.pts().or(packet.dts())
        {
            keys.push(time);
        }
    }
    set_discard(input, None);
    let _ = input.seek(0, ..);
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Have the demuxer skip every stream but `keep` (all of them with `None`
/// kept).
pub(crate) fn set_discard(input: &mut Input, keep: Option<usize>) {
    set_discard_all_but(
        input,
        &keep.map_or_else(Vec::new, |keep| vec![keep]),
        keep.is_none(),
    );
}

/// Have the demuxer skip every stream but those in `keep`, or none.
pub(crate) fn set_discard_all_but(input: &mut Input, keep: &[usize], none: bool) {
    // SAFETY: the streams exist while the context is open.
    unsafe {
        let context = &*input.as_ptr();
        for index in 0..context.nb_streams as usize {
            let stream = *context.streams.add(index);
            (*stream).discard = if none || keep.contains(&index) {
                ffi::AVDiscard::AVDISCARD_DEFAULT
            } else {
                ffi::AVDiscard::AVDISCARD_ALL
            };
        }
    }
}

#[cfg(test)]
#[path = "tests/segments.rs"]
mod tests;
