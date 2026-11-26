//! The packet queues between the demux and decode threads, and the demux
//! thread, which reads packets ahead into them and carries out seeks.

use std::collections::VecDeque;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ffmpeg::format::context::Input;
use ffmpeg::{Packet, Rescale};
use ffmpeg_next as ffmpeg;
use parking_lot::{Condvar, Mutex};

use super::{AV_TIME_BASE, Seek, SeekMode, Shared};
use crate::media::tracks::TrackKind;

/// How long the packets of subtitles not playing are kept from before the
/// next packet to decode, so a track picked shows what's on now without a
/// seek, and at most how many of each track.
const SPARE_SUBTITLE_SECONDS: f64 = 10.0;
const SPARE_SUBTITLE_PACKETS: usize = 128;
/// How far the demux thread reads ahead of the decoders.
pub(super) const FILE_READ_AHEAD: ReadAhead = ReadAhead {
    seconds: 1.0,
    bytes: 16 * 1024 * 1024,
};
pub(super) const STREAM_READ_AHEAD: ReadAhead = ReadAhead {
    seconds: 15.0,
    bytes: 48 * 1024 * 1024,
};
/// Hard caps on queued packets, reached only when one stream starves while
/// another one is far ahead (badly interleaved files).
const MAX_QUEUED_PACKETS: usize = 8192;
const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;
/// How far a skip's target can be from the keyframe before it and still be
/// landed on exactly; further, it lands on the nearest keyframe its way
/// instead of decoding everything in between, in seconds.
const EXACT_SKIP: f64 = 1.0;
/// Packets read after a seek to find the video's first, at most.
const PEEK_PACKETS: usize = 512;
/// How long a network read that has nothing yet waits to try again.
const READ_AGAIN_WAIT: Duration = Duration::from_millis(4);

pub(super) fn packet_time(packet: &Packet, time_base: f64) -> Option<f64> {
    packet
        .pts()
        .or(packet.dts())
        .map(|ts| ts as f64 * time_base)
}

#[derive(Clone, Copy)]
pub(super) struct ReadAhead {
    seconds: f64,
    bytes: usize,
}

/// Packets read ahead for one stream.
pub(super) struct PacketQueue {
    index: usize,
    time_base: f64,
    packets: VecDeque<Packet>,
}

impl PacketQueue {
    pub(super) fn new(index: usize, time_base: f64) -> Self {
        Self {
            index,
            time_base,
            packets: VecDeque::new(),
        }
    }

    /// When the next packet starts, in seconds.
    fn front_time(&self) -> Option<f64> {
        let packet = self.packets.front()?;
        Some(packet.dts().or(packet.pts())? as f64 * self.time_base)
    }

    /// Forget the packets over before `time`, and the oldest past `max`.
    fn keep_from(&mut self, time: f64, max: usize) {
        while let Some(packet) = self.packets.front() {
            let end = packet.pts().or(packet.dts()).map(|start| {
                start.saturating_add(packet.duration().max(0)) as f64 * self.time_base
            });
            if self.packets.len() > max || end.is_some_and(|end| end < time) {
                self.packets.pop_front();
            } else {
                break;
            }
        }
    }

    /// Seconds between the first and the last queued packet.
    fn span(&self) -> f64 {
        let time = |p: &Packet| p.dts().or(p.pts());
        match (
            self.packets.front().and_then(time),
            self.packets.back().and_then(time),
        ) {
            (Some(first), Some(last)) => last.saturating_sub(first).max(0) as f64 * self.time_base,
            // No timestamps: guess from the count.
            _ => self.packets.len() as f64 / 50.0,
        }
    }
}

pub(super) struct Packets {
    pub(super) video: Option<PacketQueue>,
    pub(super) audio: Option<PacketQueue>,
    /// Decoded as soon as they are read, so they don't count towards
    /// read-ahead.
    pub(super) subtitle: Option<PacketQueue>,
    /// The latest packets of each subtitle stream, kept for when it's
    /// picked; the one playing takes its packets from `subtitle` instead.
    pub(super) spare: Vec<PacketQueue>,
    pub(super) cover_index: Option<usize>,
    pub(super) cover: Option<Packet>,
    pub(super) cover_seen: bool,
    pub(super) bytes: usize,
    /// The seek generation the queued packets belong to.
    pub(super) serial: u64,
    pub(super) eof: bool,
    /// A seek for the demux thread.
    pub(super) seek: Option<Seek>,
    /// Where the last seek landed, once it has, when that isn't where it
    /// was asked to: the keyframe's time on the file's timeline. Taken by
    /// the decode thread.
    pub(super) landed: Option<Option<f64>>,
    pub(super) stopped: bool,
}

impl Packets {
    fn queue(&mut self, kind: TrackKind) -> &mut Option<PacketQueue> {
        match kind {
            TrackKind::Video => &mut self.video,
            TrackKind::Audio => &mut self.audio,
            TrackKind::Subtitle => &mut self.subtitle,
        }
    }

    fn queues(&self) -> impl Iterator<Item = &PacketQueue> {
        self.video.iter().chain(self.audio.iter())
    }

    fn wants_more(&self, limits: ReadAhead) -> bool {
        let count: usize = self.queues().map(|q| q.packets.len()).sum();
        if self.eof || count >= MAX_QUEUED_PACKETS {
            return false;
        }
        let starving = self.queues().any(|q| q.packets.is_empty());
        let short = self.queues().any(|q| q.span() < limits.seconds);
        (starving && self.bytes < MAX_QUEUED_BYTES) || (short && self.bytes < limits.bytes)
    }

    /// Seconds that can be played from queued packets alone.
    fn read_ahead(&self) -> f64 {
        if self.eof {
            return f64::INFINITY;
        }
        self.queues()
            .map(PacketQueue::span)
            .fold(f64::INFINITY, f64::min)
    }

    /// Queue `packet`; whether the decode thread may be waiting for it,
    /// its queue having been empty.
    fn push(&mut self, packet: Packet) -> bool {
        let index = packet.stream();
        let queue = [&mut self.video, &mut self.audio, &mut self.subtitle]
            .into_iter()
            .flatten()
            .find(|q| q.index == index);
        if let Some(queue) = queue {
            self.bytes += packet.size();
            let waited = queue.packets.is_empty();
            queue.packets.push_back(packet);
            return waited;
        }
        if self.cover_index == Some(index) {
            if !self.cover_seen {
                self.cover_seen = true;
                self.cover = Some(packet);
                return true;
            }
            return false;
        }
        // Kept from a little before the next packet to decode, or this one.
        let Some(start) = packet.pts().or(packet.dts()) else {
            return false;
        };
        let next = self
            .queues()
            .filter_map(PacketQueue::front_time)
            .reduce(f64::min);
        if let Some(spare) = self.spare.iter_mut().find(|q| q.index == index) {
            let next = next.unwrap_or(start as f64 * spare.time_base);
            spare.packets.push_back(packet);
            spare.keep_from(next - SPARE_SUBTITLE_SECONDS, SPARE_SUBTITLE_PACKETS);
        }
        false
    }

    fn clear(&mut self) {
        for queue in [&mut self.video, &mut self.audio, &mut self.subtitle]
            .into_iter()
            .flatten()
            .chain(&mut self.spare)
        {
            queue.packets.clear();
        }
        self.bytes = 0;
        self.eof = false;
    }
}

pub(super) enum Next {
    Packet(Packet),
    /// No more packets until the next seek.
    End,
    /// None queued, and none read until the other stream's are taken: the
    /// demux thread has read as far ahead as it may, and the next of this
    /// one, if any, is further on.
    Later,
    /// Nothing queued yet.
    Wait,
}

/// The packet queues between the demux thread and the decode thread.
pub(super) struct Demuxer {
    pub(super) packets: Mutex<Packets>,
    /// Wakes the demux thread when there's room, a seek or a stop.
    pub(super) wake: Condvar,
    pub(super) limits: ReadAhead,
    pub(super) shared: Arc<Shared>,
}

impl Demuxer {
    fn publish(&self, packets: &Packets) {
        self.shared.set_read_ahead(packets.read_ahead());
    }

    pub(super) fn pop(&self, kind: TrackKind, serial: u64) -> Next {
        let mut packets = self.packets.lock();
        if packets.serial != serial {
            // The demux thread hasn't got to the seek yet.
            return Next::Wait;
        }
        let next = packets
            .queue(kind)
            .as_mut()
            .and_then(|q| q.packets.pop_front());
        match next {
            Some(packet) => {
                packets.bytes -= packet.size();
                self.publish(&packets);
                self.wake.notify_one();
                Next::Packet(packet)
            }
            None if packets.eof => Next::End,
            None if !packets.wants_more(self.limits) => Next::Later,
            None => Next::Wait,
        }
    }

    /// Queue packets of another stream for a kind of track, or none.
    /// Subtitles start with the packets kept of them.
    pub(super) fn select(&self, kind: TrackKind, mut queue: Option<PacketQueue>) {
        let mut packets = self.packets.lock();
        if let Some(queue) = &mut queue
            && let Some(spare) = packets.spare.iter_mut().find(|q| q.index == queue.index)
        {
            queue.packets = std::mem::take(&mut spare.packets);
            packets.bytes += queue.packets.iter().map(Packet::size).sum::<usize>();
        }
        if let Some(old) = std::mem::replace(packets.queue(kind), queue) {
            packets.bytes -= old.packets.iter().map(Packet::size).sum::<usize>();
        }
        self.publish(&packets);
        self.wake.notify_one();
    }

    pub(super) fn take_cover(&self) -> Option<Packet> {
        self.packets.lock().cover.take()
    }

    /// Give what is queued, and read next, the serial of a scrub that
    /// decodes on rather than seeking. Not while a seek is under way.
    pub(super) fn renumber(&self, serial: u64) -> bool {
        let mut packets = self.packets.lock();
        if packets.seek.is_some() {
            return false;
        }
        packets.serial = serial;
        true
    }

    pub(super) fn seek(&self, seek: Seek) {
        let mut packets = self.packets.lock();
        packets.clear();
        packets.seek = Some(seek);
        packets.landed = None;
        self.publish(&packets);
        self.wake.notify_one();
    }

    /// Once the demux thread has carried out the seek for `serial`: the
    /// keyframe it landed on, if not where it was asked to.
    pub(super) fn landed(&self, serial: u64) -> Option<Option<f64>> {
        let mut packets = self.packets.lock();
        if packets.serial != serial {
            return None;
        }
        Some(packets.landed.take().flatten())
    }

    fn stop(&self) {
        self.packets.lock().stopped = true;
        self.wake.notify_one();
    }
}

pub(super) struct StopOnDrop(pub(super) Arc<Demuxer>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// A file whose name starts so has its demux thread panic once it's open,
/// as a bug would make it: the tests' only way to have one.
#[cfg(test)]
pub const PANICS_IN_DEMUX: &str = "panics-in-demux";

/// The demux thread: reads packets until the queues are full enough, and
/// carries out seeks; a live stream's to where it is now, opened again
/// with `reopen`.
pub(super) fn demux(
    mut input: Input,
    reopen: impl Fn() -> anyhow::Result<Input>,
    demuxer: &Demuxer,
    name: &str,
) {
    #[cfg(test)]
    if name.starts_with(PANICS_IN_DEMUX) {
        panic!("the demux thread failing, as the test asked");
    }
    let stopping = || demuxer.shared.stopped();
    loop {
        let seek = {
            let mut packets = demuxer.packets.lock();
            loop {
                if packets.stopped || stopping() {
                    return;
                }
                if packets.seek.is_some() || packets.wants_more(demuxer.limits) {
                    break;
                }
                demuxer.wake.wait(&mut packets);
            }
            packets.seek
        };

        if let Some(seek) = seek {
            let (video, sound) = {
                let packets = demuxer.packets.lock();
                (
                    packets.video.as_ref().map(|q| (q.index, q.time_base)),
                    packets.audio.as_ref().map(|q| (q.index, q.time_base)),
                )
            };
            let landing = if seek.mode == SeekMode::Live {
                go_live(&mut input, &reopen, video.or(sound), name)
            } else {
                land(&mut input, seek, video, sound.map(|(index, _)| index), name)
            };
            let mut packets = demuxer.packets.lock();
            // A newer seek may have come in meanwhile; it runs next time round.
            if packets.seek == Some(seek) {
                packets.seek = None;
                packets.serial = seek.serial;
                packets.landed = Some(landing.keyframe);
                for packet in landing.packets {
                    packets.push(packet);
                }
                match landing.error {
                    None => {}
                    Some(_) if stopping() => return,
                    Some(ffmpeg::Error::Eof) => packets.eof = true,
                    Some(err) => {
                        log::warn!("{name}: read error, treating it as the end: {err}");
                        packets.eof = true;
                    }
                }
                demuxer.publish(&packets);
                drop(packets);
                demuxer.shared.ring();
            }
            continue;
        }

        let mut packet = Packet::empty();
        let result = packet.read(&mut input);
        let mut packets = demuxer.packets.lock();
        if packets.seek.is_some() {
            // Read from before the seek position.
            continue;
        }
        let waited = match result {
            Ok(()) => packets.push(packet),
            Err(ffmpeg::Error::Eof) => {
                packets.eof = true;
                true
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                drop(packets);
                thread::sleep(READ_AGAIN_WAIT);
                continue;
            }
            Err(_) if stopping() => return,
            Err(err) => {
                log::warn!("{name}: read error, treating it as the end: {err}");
                packets.eof = true;
                true
            }
        };
        demuxer.publish(&packets);
        drop(packets);
        if waited {
            demuxer.shared.ring();
        }
    }
}

/// What carrying out a seek read.
#[derive(Default)]
struct Landing {
    /// Read to see where it landed, in the file's order.
    packets: Vec<Packet>,
    /// The keyframe landed on, on the file's timeline, when that isn't
    /// where the seek was asked to.
    keyframe: Option<f64>,
    /// What stopped the reading short.
    error: Option<ffmpeg::Error>,
}

/// Seek `input` to the keyframe before the target. A skip whose target is
/// far past it lands on that keyframe instead when going back, or on the
/// one after the target when going forward, so it shows at once. Past
/// where the `video` stream says it ends, the seek goes by the `sound`
/// stream: Matroska seeks by the picture's cues, and the cluster of its
/// last one can come after most of the sound.
fn land(
    input: &mut Input,
    seek: Seek,
    video: Option<(usize, f64)>,
    sound: Option<usize>,
    name: &str,
) -> Landing {
    let target = seek.target;
    let seconds = target as f64 / AV_TIME_BASE;
    let by = sound.filter(|_| video.is_some_and(|(index, _)| ends_before(input, index, seconds)));
    if let Err(err) = seek_before(input, target, by) {
        log::warn!("{name}: seek to {seconds:.2}s failed: {err}");
    }
    let (SeekMode::Skip { forward }, Some(video)) = (seek.mode, video) else {
        return Landing::default();
    };
    let mut landing = read_to(input, video, false);
    let Some(keyframe) = landing.keyframe.filter(|k| seconds - k > EXACT_SKIP) else {
        landing.keyframe = None;
        return landing;
    };
    if !forward {
        landing.keyframe = Some(keyframe);
        return landing;
    }
    if input.seek(target, target..).is_ok() {
        let mut after = read_to(input, video, false);
        if after.keyframe.is_some_and(|k| k >= seconds) {
            return after;
        }
        // Where it went isn't past the target after all: decode from there.
        after.keyframe = None;
        return after;
    }
    // No keyframe after it: back to the one before, to decode up to it.
    if let Err(err) = input.seek(target, ..target) {
        log::warn!("{name}: seek to {seconds:.2}s failed: {err}");
    }
    Landing::default()
}

/// Seek `input` to the keyframe before `target`, in `AV_TIME_BASE` units:
/// stream `by`'s, or that of the stream FFmpeg picks.
fn seek_before(input: &mut Input, target: i64, by: Option<usize>) -> Result<(), ffmpeg::Error> {
    let Some(index) = by else {
        return input.seek(target, ..target);
    };
    let stream = input.stream(index).ok_or(ffmpeg::Error::StreamNotFound)?;
    let at = target.rescale(ffmpeg::rescale::TIME_BASE, stream.time_base());
    let index = i32::try_from(index).map_err(|_| ffmpeg::Error::StreamNotFound)?;
    // SAFETY: `input` owns an open format context, and `index` is one of its
    // streams, so `at` is in that stream's time base, as FFmpeg asks.
    let sought =
        unsafe { ffmpeg::ffi::avformat_seek_file(input.as_mut_ptr(), index, i64::MIN, at, at, 0) };
    if sought < 0 {
        Err(ffmpeg::Error::from(sought))
    } else {
        Ok(())
    }
}

/// Whether stream `index` of `input` says it ends before `seconds`:
/// Matroska's muxers tag each track with where it ends, from 0.
fn ends_before(input: &Input, index: usize, seconds: f64) -> bool {
    input
        .stream(index)
        .and_then(|stream| ffmpeg_safe::tag(&stream.metadata(), c"DURATION"))
        .and_then(|end| length_in_seconds(&end))
        .is_some_and(|end| end < seconds)
}

/// The seconds in a length written `HH:MM:SS.fraction`, as Matroska tags
/// one.
fn length_in_seconds(text: &str) -> Option<f64> {
    let mut parts = text.trim().splitn(3, ':');
    let hours: u32 = parts.next()?.parse().ok()?;
    let minutes: u32 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    let length = f64::from(hours) * 3600.0 + f64::from(minutes) * 60.0 + seconds;
    (seconds >= 0.0 && length.is_finite()).then_some(length)
}

/// Open a live stream's `input` again with `reopen`, where it is now, and
/// read to the first keyframe of the `first` stream (index, time base):
/// the picture's, else the sound's. If it won't open as it was, it reads
/// on from where it was, to its next keyframe.
fn go_live(
    input: &mut Input,
    reopen: impl Fn() -> anyhow::Result<Input>,
    first: Option<(usize, f64)>,
    name: &str,
) -> Landing {
    match reopen() {
        Ok(again) if same_streams(input, &again) => *input = again,
        Ok(_) => log::warn!("{name}: its streams changed, so it plays on from where it was"),
        Err(err) => {
            log::warn!("{name}: it plays on from where it was, as it won't open again: {err:#}")
        }
    }
    first.map_or_else(Landing::default, |first| read_to(input, first, true))
}

/// Whether `a` and `b` have the same streams, by kind and codec.
fn same_streams(a: &Input, b: &Input) -> bool {
    let streams = |input: &Input| -> Vec<_> {
        input
            .streams()
            .map(|stream| {
                let parameters = stream.parameters();
                (parameters.medium(), parameters.id())
            })
            .collect()
    };
    streams(a) == streams(b)
}

/// Read up to the first packet of `stream` (index, time base), or its
/// first keyframe if `key`, keeping its time in `keyframe`: with no
/// keyframe among the packets peeked, the time of its first.
fn read_to(input: &mut Input, stream: (usize, f64), key: bool) -> Landing {
    let mut landing = Landing::default();
    let mut first = None;
    while landing.packets.len() < PEEK_PACKETS {
        let mut packet = Packet::empty();
        match packet.read(input) {
            Ok(()) => {
                let time = (packet.stream() == stream.0).then(|| packet_time(&packet, stream.1));
                let found = time.is_some() && (!key || packet.is_key());
                first = first.or(time);
                landing.packets.push(packet);
                if found {
                    landing.keyframe = time.flatten();
                    return landing;
                }
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                thread::sleep(READ_AGAIN_WAIT);
            }
            Err(err) => {
                landing.error = Some(err);
                break;
            }
        }
    }
    landing.keyframe = first.flatten();
    landing
}

#[cfg(test)]
#[path = "tests/demux.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/demux_properties.rs"]
mod properties;
