//! Making HLS segments and MPEG-TS streams: reading the source from where
//! asked, copying or re-encoding each track, and writing it out, cut where
//! segments start.
//!
//! A file's segments are made by a job running a little ahead of what the
//! device asks for; asking for one far from it (a seek) starts a new job
//! there. Each segment is cut the same way whichever job makes it, so
//! segments from different jobs fit together. A live stream has one job
//! from where it is now, keeping the last few segments. A progressive
//! stream is made for each request, from where it asks.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use ffmpeg::format::context::Input;
use ffmpeg::{Packet, Rational, codec, ffi, frame};
use ffmpeg_next as ffmpeg;

use crate::Error;
use crate::adjust::{self, Adjustments};
use crate::encode::{
    AudioEncoder, AudioSettings, Colors, VideoEncoder, VideoSettings, kept_colors,
};
use crate::media::{self, MediaInfo, Source};
use crate::mux::{Format, Muxer, OutStream};
use crate::overlay::{Coding, Overlay};
use crate::plan::{AudioPlan, Plan, VideoPlan};
use crate::playlist;
use crate::profile::Segments;
use crate::segments::{Domain, Timeline, set_discard_all_but};
use crate::sync::{self, lock};

/// How close a timestamp may fall short of a segment's start and still be
/// taken as at it, in seconds.
const EPSILON: f64 = 1e-3;
/// Segments made ahead of the last one asked for.
const AHEAD: usize = 3;
/// How far past the segment a job makes next a request may be and still
/// wait for it, rather than start a job of its own.
const REACH: usize = 2;
/// Segments kept behind the last one asked for, for players that ask twice.
const BEHIND: usize = 4;
/// How long a request waits for what it asks for.
const WAIT: Duration = Duration::from_secs(60);
/// Segments a live playlist lists, and how many more are kept for players
/// a little behind.
const LIVE_WINDOW: usize = 6;
const LIVE_KEPT: usize = 10;
/// Packets written to a progressive stream between sends.
const STREAM_BATCH: usize = 24;

/// What a cast's jobs make.
pub(crate) struct Media {
    pub source: Source,
    pub info: MediaInfo,
    pub plan: Plan,
    pub kind: Segments,
    /// How long segments are, about, in seconds.
    pub segment: f64,
    /// Seconds picture subtitles drawn in show after their times.
    pub subtitle_delay: f64,
}

/// Wait for `changed` to be rung, until `deadline` at the most; past it,
/// a request fails as `late` says.
fn wait_until<'a, T>(
    changed: &Condvar,
    state: MutexGuard<'a, T>,
    deadline: Instant,
    late: &str,
) -> Result<MutexGuard<'a, T>, Error> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(Error::Mux(late.into()));
    }
    Ok(sync::wait_timeout(changed, state, left))
}

/// Reads that fail partway, for the tests: a source's location, and how
/// many packets the next job reading it gets before its reads fail as a
/// disk's or a network's might. Jobs after it read the source as it is.
#[cfg(test)]
pub(crate) static BREAKS: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());

/// A packet on its way out.
struct Out {
    packet: Packet,
    /// The output stream: video first, then sound.
    output: usize,
    /// When it shows, in seconds of the source's timeline.
    time: f64,
    key: bool,
    /// For copied video's keyframes, its timestamp in the stream's time base
    /// and the timeline's domain, to match against segment starts exactly.
    mark: Option<i64>,
}

/// Where a job starts reading.
enum Start {
    Beginning,
    /// At a keyframe of copied video (in its stream's time base and
    /// `domain`); sound from `time`.
    Key {
        key: i64,
        time: f64,
    },
    /// From `time`, in seconds; copied video from the keyframe before it.
    Time(f64),
}

/// Where re-encoded video gets keyframes, besides its first frame.
enum Keys {
    /// At these times: the segments' starts.
    At(Vec<f64>, usize),
    /// Every `period` seconds.
    Every { period: f64, next: Option<f64> },
}

impl Keys {
    /// Whether a frame at `time` is to be a keyframe.
    fn due(&mut self, time: f64) -> bool {
        match self {
            Self::At(times, next) => {
                let mut due = false;
                while *next < times.len() && time >= times[*next] - EPSILON {
                    *next += 1;
                    due = true;
                }
                due
            }
            Self::Every { period, next } => match next {
                Some(at) if time < *at - EPSILON => false,
                _ => {
                    *next = Some(time + *period);
                    true
                }
            },
        }
    }
}

enum Work {
    CopyVideo {
        started: bool,
        from: Option<i64>,
        domain: Domain,
    },
    CopyAudio {
        started: bool,
        from: f64,
    },
    Video {
        decoder: ffmpeg::decoder::Video,
        encoder: Box<VideoEncoder>,
        keys: Keys,
        from: f64,
        first: bool,
        last: Option<i64>,
        /// Picture subtitles drawn in.
        overlay: Option<Box<Overlay>>,
        /// What's done to the picture.
        adjustments: Option<Adjustments>,
    },
    Audio {
        decoder: ffmpeg::decoder::Audio,
        encoder: Box<AudioEncoder>,
        from: f64,
    },
}

struct Track {
    input: usize,
    output: usize,
    time_base: f64,
    /// Seconds its timestamps are moved later (negative: earlier).
    shift: f64,
    work: Work,
}

/// What reading a source failing partway comes to.
#[derive(Clone, Copy)]
enum Broken {
    /// The source ends there. For a live stream or a progressive one,
    /// which can't be made again from where it broke.
    Ends,
    /// The job fails, so the segments past it aren't made (empty, as at
    /// the source's end) but asked for again, by a job of their own.
    Fails,
}

/// Reading a source and turning its packets into output packets, written
/// through a muxer.
struct Pipeline {
    input: Input,
    muxer: Muxer,
    tracks: Vec<Track>,
    lengths: Lengths,
    /// Each output stream's latest time, for packets that have none.
    times: Vec<f64>,
    /// Seconds picture subtitles drawn in show after their times.
    subtitle_delay: f64,
    /// Packets read before reads fail, from `BREAKS`.
    #[cfg(test)]
    reads_left: Option<usize>,
}

/// Each output stream's latest packet, held until the next one says how
/// long it lasts, when it doesn't say itself: an MP4 fragment's last
/// sample needs it, or the next fragment starts early.
struct Lengths {
    held: Vec<Option<Out>>,
    /// Each stream's last length known, for its very last packet.
    last: Vec<i64>,
}

impl Lengths {
    fn new(streams: usize) -> Self {
        Self {
            held: (0..streams).map(|_| None).collect(),
            last: vec![0; streams],
        }
    }

    /// Hold `out`, and pass on its stream's previous packet.
    fn pass(&mut self, out: Out) -> Option<Out> {
        let output = out.output;
        let previous = self.held[output].replace(out).map(|mut previous| {
            let next = self.held[output].as_ref().expect("just held");
            if previous.packet.duration() <= 0
                && let (Some(this), Some(next)) = (
                    previous.packet.dts().or(previous.packet.pts()),
                    next.packet.dts().or(next.packet.pts()),
                )
                && next > this
            {
                previous.packet.set_duration(next - this);
            }
            previous
        })?;
        if previous.packet.duration() > 0 {
            self.last[output] = previous.packet.duration();
        }
        Some(previous)
    }

    /// The packets still held, at the end, each as long as its stream's
    /// last one when it doesn't say.
    fn rest(&mut self) -> Vec<Out> {
        let last = &self.last;
        self.held
            .iter_mut()
            .enumerate()
            .filter_map(|(output, held)| {
                let mut out = held.take()?;
                if out.packet.duration() <= 0 {
                    out.packet.set_duration(last[output]);
                }
                Some(out)
            })
            .collect()
    }
}

// SAFETY: the pipeline is made and used by one job thread at a time.
unsafe impl Send for Pipeline {}

impl Pipeline {
    /// Open the source, seek to `start`, set up each track and write the
    /// output's header.
    fn open(
        media: &Media,
        start: Start,
        keys: Keys,
        domain: Domain,
        stop: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        let mut input = media::open(&media.source, stop)?;
        let video = media.plan.video_stream();
        let audio = media.plan.audio_stream();
        let keep: Vec<usize> = video
            .into_iter()
            .chain(audio)
            .chain(media.plan.subtitles)
            .collect();
        set_discard_all_but(&mut input, &keep, false);
        // Sound sent later is read from earlier on.
        let delay = media.plan.audio_delay;
        seek(&mut input, &start, video, delay.max(0.0))?;

        let (from_key, from_time) = match start {
            Start::Beginning => (None, f64::NEG_INFINITY),
            Start::Key { key, time } => (Some(key), time),
            Start::Time(time) => (None, time),
        };
        let mut tracks = Vec::new();
        let mut outputs = Vec::new();
        if let Some(plan) = &media.plan.video {
            let (stream_index, time_base, work, out) =
                video_work(media, &input, plan, from_key, from_time, keys, domain)?;
            tracks.push(Track {
                input: stream_index,
                output: outputs.len(),
                time_base: f64::from(time_base),
                shift: 0.0,
                work,
            });
            outputs.push(out);
        }
        if let Some(plan) = &media.plan.audio {
            // Sound sent earlier starts no sooner than the media.
            let from = if delay < 0.0 {
                from_time.max(media.info.start)
            } else {
                from_time
            };
            let (stream_index, time_base, work, out) = audio_work(media, &input, plan, from)?;
            tracks.push(Track {
                input: stream_index,
                output: outputs.len(),
                time_base: f64::from(time_base),
                shift: delay,
                work,
            });
            outputs.push(out);
        }
        let mut muxer = Muxer::new(media.plan.format(), &outputs)?;
        muxer.write_header()?;
        let count = outputs.len();
        Ok(Self {
            input,
            muxer,
            tracks,
            lengths: Lengths::new(count),
            times: vec![0.0; count],
            subtitle_delay: media.subtitle_delay,
            #[cfg(test)]
            reads_left: {
                let location = media.source.location();
                let mut breaks = lock(&BREAKS);
                let at = breaks.iter().position(|(source, _)| *source == location);
                at.map(|at| breaks.remove(at).1)
            },
        })
    }

    /// Read the next packet and turn it into output packets; false at the
    /// end of the source, or where reading it fails if `broken` says so.
    fn step(&mut self, outs: &mut Vec<Out>, broken: Broken) -> Result<bool, Error> {
        let mut packet = Packet::empty();
        let read = packet.read(&mut self.input);
        #[cfg(test)]
        let read = match &mut self.reads_left {
            Some(0) => Err(ffmpeg::Error::Other {
                errno: ffmpeg::util::error::EIO,
            }),
            Some(left) => {
                *left -= 1;
                read
            }
            None => read,
        };
        match read {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => {
                self.flush(outs)?;
                return Ok(false);
            }
            Err(ffmpeg::Error::Exit) => return Err(Error::Source("stopped".into())),
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                thread::sleep(Duration::from_millis(10));
                return Ok(true);
            }
            Err(err) => match broken {
                Broken::Ends => {
                    // As FFmpeg's own tools do: what was read plays, to here.
                    log::warn!("reading the media stopped: {err}");
                    self.flush(outs)?;
                    return Ok(false);
                }
                Broken::Fails => {
                    return Err(Error::Source(format!("reading the media failed: {err}")));
                }
            },
        }
        let index = packet.stream();
        for track in &mut self.tracks {
            if let Work::Video {
                overlay: Some(overlay),
                ..
            } = &mut track.work
                && overlay.stream() == index
            {
                overlay.take(&packet);
            }
        }
        if let Some(track) = self.tracks.iter().position(|track| track.input == index) {
            self.process(track, Some(packet), outs)?;
        }
        Ok(true)
    }

    /// Everything still in the decoders, encoders and held back.
    fn flush(&mut self, outs: &mut Vec<Out>) -> Result<(), Error> {
        for track in 0..self.tracks.len() {
            self.process(track, None, outs)?;
        }
        outs.extend(self.lengths.rest());
        Ok(())
    }

    /// Turn a packet of a track (or, with `None`, the end of it) into
    /// output packets.
    fn process(
        &mut self,
        track: usize,
        packet: Option<Packet>,
        outs: &mut Vec<Out>,
    ) -> Result<(), Error> {
        let mut made = Vec::new();
        let Track {
            output,
            time_base,
            shift,
            work,
            ..
        } = &mut self.tracks[track];
        let (output, time_base, shift) = (*output, *time_base, *shift);
        let subtitle_delay = self.subtitle_delay;
        match work {
            Work::CopyVideo {
                started,
                from,
                domain,
            } => made.extend(copy_video(packet, started, *from, *domain, time_base)),
            Work::CopyAudio { started, from } => {
                made.extend(copy_audio(packet, started, *from, time_base, shift));
            }
            Work::Video {
                decoder,
                encoder,
                keys,
                from,
                first,
                last,
                overlay,
                adjustments,
            } => {
                let end = packet.is_none();
                send(decoder, packet.as_ref(), "video");
                let mut decoded = frame::Video::empty();
                while decoder.receive_frame(&mut decoded).is_ok() {
                    let Some(ts) = decoded.timestamp().or(decoded.pts()) else {
                        continue;
                    };
                    let time = ts as f64 * time_base;
                    if time < *from - EPSILON {
                        continue;
                    }
                    let due = keys.due(time);
                    let pts = match *last {
                        Some(last) if ts <= last => last + 1,
                        _ => ts,
                    };
                    *last = Some(pts);
                    let mut frame = prepared(encoder, &mut decoded, adjustments.as_ref())?;
                    if let Some(overlay) = overlay {
                        overlay.draw(&mut frame, time - subtitle_delay);
                    }
                    encoder.encode(frame, pts, *first || due)?;
                    *first = false;
                    drain_video(encoder, &mut made);
                }
                if end {
                    encoder.send_eof()?;
                    drain_video(encoder, &mut made);
                }
            }
            Work::Audio {
                decoder,
                encoder,
                from,
            } => {
                let end = packet.is_none();
                send(decoder, packet.as_ref(), "sound");
                let mut decoded = frame::Audio::empty();
                while decoder.receive_frame(&mut decoded).is_ok() {
                    let Some(ts) = decoded.timestamp().or(decoded.pts()) else {
                        continue;
                    };
                    encoder.send(&mut decoded, ts as f64 * time_base + shift, *from)?;
                    drain_audio(encoder, &mut made);
                }
                if end {
                    encoder.send_eof()?;
                    drain_audio(encoder, &mut made);
                }
            }
        }
        for (time, key, mark, packet) in made {
            let time = time.unwrap_or(self.times[output]);
            self.times[output] = time;
            let out = Out {
                packet,
                output,
                time,
                key,
                mark,
            };
            outs.extend(self.lengths.pass(out));
        }
        Ok(())
    }

    fn write(&mut self, out: Out) -> Result<(), Error> {
        self.muxer.write(out.packet, out.output)
    }
}

/// An output packet a track made: when it shows, in seconds, whether it
/// is a keyframe, its mark (see `Out`) and the packet.
type Made = (Option<f64>, bool, Option<i64>, Packet);

/// A copied video packet to pass on, from a keyframe on: the one at `from`,
/// in `domain`, or the first.
fn copy_video(
    packet: Option<Packet>,
    started: &mut bool,
    from: Option<i64>,
    domain: Domain,
    time_base: f64,
) -> Option<Made> {
    let packet = packet?;
    let in_domain = match domain {
        Domain::Dts => packet.dts().or(packet.pts()),
        Domain::Pts => packet.pts().or(packet.dts()),
    };
    if !*started {
        // From a keyframe, the one asked for or the first.
        if !packet.is_key() || from.is_some_and(|from| in_domain.is_none_or(|ts| ts < from)) {
            return None;
        }
        *started = true;
    }
    let key = packet.is_key();
    Some((
        packet
            .pts()
            .or(packet.dts())
            .map(|ts| ts as f64 * time_base),
        key,
        in_domain.filter(|_| key),
        packet,
    ))
}

/// `decoded` as `encoder` takes it: at its size and in its format, with
/// `adjustments` done.
fn prepared(
    encoder: &mut VideoEncoder,
    decoded: &mut frame::Video,
    adjustments: Option<&Adjustments>,
) -> Result<frame::Video, Error> {
    let Some(adjustments) = adjustments else {
        return encoder.scale(decoded);
    };
    adjust::crop(decoded, adjustments);
    let (width, height) = encoder.size();
    let (width, height) = adjust::before_turning(adjustments, width, height);
    let scaled = encoder.scale_to(decoded, width, height)?;
    Ok(adjust::finish(scaled, adjustments))
}

/// A copied sound packet to pass on, `shift` seconds later, from `from`
/// seconds on.
fn copy_audio(
    packet: Option<Packet>,
    started: &mut bool,
    from: f64,
    time_base: f64,
    shift: f64,
) -> Option<Made> {
    let mut packet = packet?;
    let time = packet
        .pts()
        .or(packet.dts())
        .map(|ts| ts as f64 * time_base + shift);
    if !*started {
        if time.is_none_or(|time| time < from - EPSILON) {
            return None;
        }
        *started = true;
    }
    let by = (shift / time_base).round() as i64;
    if by != 0 {
        packet.set_pts(packet.pts().map(|ts| ts + by));
        packet.set_dts(packet.dts().map(|ts| ts + by));
    }
    Some((time, packet.is_key(), None, packet))
}

/// Give a decoder the next packet of its `what` track, or with `None`,
/// tell it the track has ended.
fn send(decoder: &mut ffmpeg::decoder::Opened, packet: Option<&Packet>, what: &str) {
    match packet {
        Some(packet) => {
            if let Err(err) = decoder.send_packet(packet) {
                log::debug!("a {what} packet didn't decode: {err}");
            }
        }
        None => {
            let _ = decoder.send_eof();
        }
    }
}

fn drain_video(encoder: &mut VideoEncoder, made: &mut Vec<Made>) {
    let time_base = f64::from(encoder.time_base());
    let mut packet = Packet::empty();
    while encoder.receive(&mut packet) {
        made.push((
            packet.pts().map(|pts| pts as f64 * time_base),
            packet.is_key(),
            None,
            std::mem::replace(&mut packet, Packet::empty()),
        ));
    }
}

fn drain_audio(encoder: &mut AudioEncoder, made: &mut Vec<Made>) {
    let time_base = f64::from(encoder.time_base());
    let mut packet = Packet::empty();
    while encoder.receive(&mut packet) {
        made.push((
            packet.pts().map(|pts| pts as f64 * time_base),
            true,
            None,
            std::mem::replace(&mut packet, Packet::empty()),
        ));
    }
}

/// The video track's work and output stream, and its stream's index and
/// time base: copied from the keyframe `from_key`, or re-encoded from
/// `from_time`.
fn video_work(
    media: &Media,
    input: &Input,
    plan: &VideoPlan,
    from_key: Option<i64>,
    from_time: f64,
    keys: Keys,
    domain: Domain,
) -> Result<(usize, Rational, Work, OutStream), Error> {
    let global_header = media.plan.format() == Format::Fmp4;
    let (stream_index, parameters, time_base, frame_rate) = stream_details(input, plan)?;
    let (work, out) = match plan {
        VideoPlan::Copy { .. } => (
            Work::CopyVideo {
                started: false,
                from: from_key,
                domain,
            },
            OutStream {
                parameters,
                time_base,
            },
        ),
        VideoPlan::Encode {
            stream,
            to,
            encoder,
            width,
            height,
            ten_bit,
            hdr,
            bit_rate,
            ..
        } => {
            let decoder = open_video_decoder(&parameters, time_base)?;
            let info = media.info.video(*stream);
            let reshapes = info.is_some_and(|video| video.ipt);
            let colors = kept_colors(Colors::of(&parameters), *ten_bit, *hdr, reshapes);
            let size = info.map_or((*width, *height), |video| (video.width, video.height));
            let overlay = media.plan.subtitles.and_then(|subtitles| {
                // Coded as what's sent is, at its height.
                Overlay::open(input, subtitles, size, Coding::of(colors.as_ref(), *height))
                    .inspect_err(|err| log::warn!("the subtitles can't be drawn in: {err}"))
                    .ok()
            });
            let encoder = VideoEncoder::open(&VideoSettings {
                encoder,
                codec: *to,
                width: *width,
                height: *height,
                ten_bit: *ten_bit,
                bit_rate: *bit_rate,
                time_base,
                frame_rate,
                colors,
                reshapes,
                global_header,
            })?;
            let out = OutStream {
                parameters: encoder.parameters(),
                time_base: encoder.time_base(),
            };
            (
                Work::Video {
                    decoder,
                    encoder: Box::new(encoder),
                    keys,
                    from: from_time,
                    first: true,
                    last: None,
                    overlay: overlay.map(Box::new),
                    adjustments: media.plan.adjustments,
                },
                out,
            )
        }
    };
    Ok((stream_index, time_base, work, out))
}

/// The sound track's work and output stream, and its stream's index and
/// time base, from `from_time`.
fn audio_work(
    media: &Media,
    input: &Input,
    plan: &AudioPlan,
    from_time: f64,
) -> Result<(usize, Rational, Work, OutStream), Error> {
    let global_header = media.plan.format() == Format::Fmp4;
    let stream_index = match plan {
        AudioPlan::Copy { stream, .. } | AudioPlan::Encode { stream, .. } => *stream,
    };
    let stream = input
        .stream(stream_index)
        .ok_or_else(|| Error::Source(format!("there is no stream {stream_index}")))?;
    let (parameters, time_base) = (stream.parameters(), stream.time_base());
    let (work, out) = match plan {
        AudioPlan::Copy { .. } => (
            Work::CopyAudio {
                started: false,
                from: from_time,
            },
            OutStream {
                parameters,
                time_base,
            },
        ),
        AudioPlan::Encode {
            to,
            channels,
            sample_rate,
            bit_rate,
            ..
        } => {
            let decoder = open_audio_decoder(&parameters, time_base)?;
            let encoder = AudioEncoder::open(&AudioSettings {
                codec: *to,
                channels: *channels,
                sample_rate: *sample_rate,
                bit_rate: *bit_rate,
                global_header,
            })?;
            let out = OutStream {
                parameters: encoder.parameters(),
                time_base: encoder.time_base(),
            };
            (
                Work::Audio {
                    decoder,
                    encoder: Box::new(encoder),
                    from: from_time,
                },
                out,
            )
        }
    };
    Ok((stream_index, time_base, work, out))
}

/// The video stream's index, parameters, time base and frame rate.
fn stream_details(
    input: &Input,
    plan: &VideoPlan,
) -> Result<(usize, codec::Parameters, Rational, Option<Rational>), Error> {
    let index = match plan {
        VideoPlan::Copy { stream, .. } | VideoPlan::Encode { stream, .. } => *stream,
    };
    let stream = input
        .stream(index)
        .ok_or_else(|| Error::Source(format!("there is no stream {index}")))?;
    let rate = media::frame_rate(&stream);
    Ok((index, stream.parameters(), stream.time_base(), rate))
}

fn open_video_decoder(
    parameters: &codec::Parameters,
    time_base: Rational,
) -> Result<ffmpeg::decoder::Video, Error> {
    let fail = |err: ffmpeg::Error| Error::Source(format!("can't decode the video: {err}"));
    let mut context = codec::context::Context::from_parameters(parameters.clone()).map_err(fail)?;
    let threads = thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 8));
    // `safe` only exists with older FFmpeg versions, hence the struct update.
    #[allow(clippy::needless_update)]
    context.set_threading(codec::threading::Config {
        kind: codec::threading::Type::Frame,
        count: threads,
        ..Default::default()
    });
    let mut decoder = context.decoder();
    decoder.set_packet_time_base(time_base);
    let codec = ffmpeg::decoder::find(parameters.id())
        .ok_or_else(|| Error::Source("there is no decoder for the video".into()))?;
    decoder
        .open_as(codec)
        .and_then(|opened| opened.video())
        .map_err(fail)
}

fn open_audio_decoder(
    parameters: &codec::Parameters,
    time_base: Rational,
) -> Result<ffmpeg::decoder::Audio, Error> {
    let fail = |err: ffmpeg::Error| Error::Source(format!("can't decode the sound: {err}"));
    let context = codec::context::Context::from_parameters(parameters.clone()).map_err(fail)?;
    let mut decoder = context.decoder();
    decoder.set_packet_time_base(time_base);
    decoder.audio().map_err(fail)
}

/// Seek `input` to where `start` says, `early` seconds before it.
fn seek(input: &mut Input, start: &Start, video: Option<usize>, early: f64) -> Result<(), Error> {
    let at = |time: f64| (time * f64::from(ffi::AV_TIME_BASE)).round() as i64;
    let (stream, ts) = match *start {
        Start::Beginning => return Ok(()),
        // Before the keyframe, which copied video waits for.
        Start::Key { time, .. } if early > 0.0 => (-1, at(time - early)),
        Start::Key { key, .. } => (video.map_or(-1, |video| video as i32), key),
        Start::Time(time) => (-1, at(time - early)),
    };
    // SAFETY: the context is open; seeking to the keyframe at or before
    // `ts`.
    let result =
        unsafe { ffi::avformat_seek_file(input.as_mut_ptr(), stream, i64::MIN, ts, ts, 0) };
    if result < 0 {
        return Err(Error::Source(format!(
            "can't seek in the media: {}",
            ffmpeg::Error::from(result)
        )));
    }
    Ok(())
}

/// Whether `out` falls in a segment after `segment`: for the leading
/// stream's keyframes, whether it starts the next one.
fn past(timeline: &Timeline, segment: usize, out: &Out, leader: bool) -> bool {
    let next = segment + 1;
    if next >= timeline.len() {
        return false;
    }
    match (&timeline.keyframes, out.mark) {
        (Some((keys, _)), Some(mark)) if leader => mark >= keys[next],
        _ => out.time >= timeline.start(next) - EPSILON,
    }
}

pub(crate) enum Product {
    Init(Vec<u8>),
    Segment(usize, Vec<u8>),
}

/// Make a file's segments from `from` on, handing each to `deliver`
/// until it says to stop.
fn make_segments(
    media: &Media,
    timeline: &Timeline,
    from: usize,
    stop: &Arc<AtomicBool>,
    deliver: &mut dyn FnMut(Product) -> bool,
) -> Result<(), Error> {
    let domain = timeline
        .keyframes
        .as_ref()
        .map_or(Domain::Pts, |(_, domain)| *domain);
    let start = match (&timeline.keyframes, from) {
        (_, 0) => Start::Beginning,
        (Some((keys, _)), _) => Start::Key {
            key: keys[from],
            time: timeline.start(from),
        },
        (None, _) => Start::Time(timeline.start(from)),
    };
    let keys = Keys::At(timeline.starts[from + 1..].to_vec(), 0);
    let mut pipeline = Pipeline::open(media, start, keys, domain, Arc::clone(stop))?;
    let mut segment = from;
    let mut held: Vec<Out> = Vec::new();
    let read = read_through(&mut pipeline, stop, Broken::Fails, &mut |pipeline, out| {
        let leader = out.output == 0;
        if leader && out.key && past(timeline, segment, &out, true) {
            let bytes = pipeline.muxer.cut()?;
            let cut = Product::Segment(segment, bytes);
            if !hand_over(&mut pipeline.muxer, cut, Product::Init, deliver) {
                return Ok(false);
            }
            segment += 1;
            // A gap in the media leaves segments with nothing in them.
            while past(timeline, segment, &out, true) {
                if !deliver(Product::Segment(segment, Vec::new())) {
                    return Ok(false);
                }
                segment += 1;
            }
            for follower in held.drain(..) {
                pipeline.write(follower)?;
            }
        } else if !leader && past(timeline, segment, &out, false) {
            // It starts after this segment; it waits for the leading
            // stream's keyframe to cut it.
            held.push(out);
            return Ok(true);
        }
        pipeline.write(out)?;
        Ok(true)
    })?;
    if !read {
        return Ok(());
    }
    for follower in held {
        pipeline.write(follower)?;
    }
    let last = Product::Segment(segment, pipeline.muxer.finish()?);
    if !hand_over(&mut pipeline.muxer, last, Product::Init, deliver) {
        return Ok(());
    }
    // The media ended before the segments the timeline has after this.
    for rest in segment + 1..timeline.len() {
        if !deliver(Product::Segment(rest, Vec::new())) {
            break;
        }
    }
    Ok(())
}

/// Read the source to its end, handing each output packet to `each`, which
/// writes it or holds it for later, and says whether to go on. False if it
/// stopped before the end, for `stop` or as `each` said.
fn read_through(
    pipeline: &mut Pipeline,
    stop: &AtomicBool,
    broken: Broken,
    each: &mut dyn FnMut(&mut Pipeline, Out) -> Result<bool, Error>,
) -> Result<bool, Error> {
    let mut outs = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let more = pipeline.step(&mut outs, broken)?;
        for out in outs.drain(..) {
            if !each(pipeline, out)? {
                return Ok(false);
            }
        }
        if !more {
            return Ok(true);
        }
    }
}

/// A file's HLS segments, made as they are asked for.
pub(crate) struct Vod {
    media: Arc<Media>,
    timeline: Timeline,
    /// The media playlist and the subtitles', made when first asked for.
    playlist: OnceLock<Arc<Vec<u8>>>,
    subtitles: OnceLock<Arc<Vec<u8>>>,
    state: Mutex<VodState>,
    changed: Condvar,
}

#[derive(Default)]
struct VodState {
    init: Option<Arc<Vec<u8>>>,
    segments: BTreeMap<usize, Arc<Vec<u8>>>,
    job: Option<Running>,
    /// The last segment asked for.
    wanted: usize,
    /// Jobs started so far, each one's id.
    jobs: u64,
    /// The last job that failed, and why.
    failure: Option<(u64, String)>,
    closed: bool,
}

struct Running {
    id: u64,
    /// Where it started, for the tests to see.
    #[cfg(test)]
    from: usize,
    next: usize,
    stop: Arc<AtomicBool>,
}

impl Vod {
    pub fn new(media: Arc<Media>, timeline: Timeline) -> Arc<Self> {
        Arc::new(Self {
            media,
            timeline,
            playlist: OnceLock::new(),
            subtitles: OnceLock::new(),
            state: Mutex::default(),
            changed: Condvar::new(),
        })
    }

    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// The media playlist, every segment in it, made once.
    pub fn playlist(&self) -> Arc<Vec<u8>> {
        let made = self
            .playlist
            .get_or_init(|| Arc::new(playlist::vod(&self.timeline, self.media.kind).into_bytes()));
        Arc::clone(made)
    }

    /// The subtitles' playlist, a WebVTT file for each segment, made once.
    pub fn subtitles_playlist(&self) -> Arc<Vec<u8>> {
        let made = self
            .subtitles
            .get_or_init(|| Arc::new(playlist::subtitles(&self.timeline).into_bytes()));
        Arc::clone(made)
    }

    /// The init segment, for MP4 segments.
    pub fn init(self: &Arc<Self>) -> Result<Arc<Vec<u8>>, Error> {
        let deadline = Instant::now() + WAIT;
        let mut state = lock(&self.state);
        let mut mine = None;
        loop {
            if let Some(init) = &state.init {
                return Ok(Arc::clone(init));
            }
            if state.closed {
                return Err(Error::Source("the cast has ended".into()));
            }
            if state.job.is_none() {
                if let Some(err) = failed(&state, mine) {
                    return Err(err);
                }
                if mine.is_some() {
                    return Err(Error::Mux("the init segment wasn't made".into()));
                }
                let from = state.wanted;
                mine = Some(self.start(&mut state, from));
            }
            state = self.wait(state, deadline)?;
        }
    }

    /// Segment `segment`, made now if it isn't already.
    pub fn segment(self: &Arc<Self>, segment: usize) -> Result<Arc<Vec<u8>>, Error> {
        let deadline = Instant::now() + WAIT;
        let mut state = lock(&self.state);
        state.wanted = segment;
        self.changed.notify_all();
        let mut mine = None;
        let mut started = 0;
        loop {
            if let Some(bytes) = state.segments.get(&segment) {
                return Ok(Arc::clone(bytes));
            }
            if state.closed {
                return Err(Error::Source("the cast has ended".into()));
            }
            // Not one the job made already, which isn't kept or it'd be
            // had: the job doesn't make it again.
            let coming = state
                .job
                .as_ref()
                .is_some_and(|job| job.next <= segment && segment <= job.next + REACH);
            if !coming {
                if let Some(err) = failed(&state, mine) {
                    return Err(err);
                }
                if started == 2 {
                    return Err(Error::Mux(format!("segment {segment} wasn't made")));
                }
                mine = Some(self.start(&mut state, segment));
                started += 1;
            }
            state = self.wait(state, deadline)?;
        }
    }

    /// Stop making segments, for good.
    pub fn close(&self) {
        let mut state = lock(&self.state);
        state.closed = true;
        if let Some(job) = state.job.take() {
            job.stop.store(true, Ordering::Relaxed);
        }
        state.segments.clear();
        self.changed.notify_all();
    }

    fn wait<'a>(
        &self,
        state: MutexGuard<'a, VodState>,
        deadline: Instant,
    ) -> Result<MutexGuard<'a, VodState>, Error> {
        wait_until(
            &self.changed,
            state,
            deadline,
            "making the segment took too long",
        )
    }

    /// Start a job making segments from `from`, stopping the one before.
    fn start(self: &Arc<Self>, state: &mut VodState, from: usize) -> u64 {
        if let Some(job) = state.job.take() {
            job.stop.store(true, Ordering::Relaxed);
        }
        state.jobs += 1;
        let id = state.jobs;
        let stop = Arc::new(AtomicBool::new(false));
        state.job = Some(Running {
            id,
            #[cfg(test)]
            from,
            next: from,
            stop: Arc::clone(&stop),
        });
        let vod = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("stream-server segments".into())
            .spawn(move || vod.run(id, from, &stop));
        if let Err(err) = spawned {
            state.job = None;
            state.failure = Some((id, format!("can't start making segments: {err}")));
        }
        self.changed.notify_all();
        id
    }

    fn run(&self, id: u64, from: usize, stop: &Arc<AtomicBool>) {
        let result = make_segments(&self.media, &self.timeline, from, stop, &mut |product| {
            self.store(id, stop, product)
        });
        let mut state = lock(&self.state);
        if let Err(err) = result
            && !stop.load(Ordering::Relaxed)
        {
            log::warn!("making segments from {from} failed: {err}");
            state.failure = Some((id, err.to_string()));
        }
        if state.job.as_ref().is_some_and(|job| job.id == id) {
            state.job = None;
        }
        self.changed.notify_all();
    }

    /// Keep what a job made; then, with a segment, wait while the job is
    /// far enough ahead. False when the job is to stop.
    fn store(&self, id: u64, stop: &AtomicBool, product: Product) -> bool {
        let mut state = lock(&self.state);
        if stop.load(Ordering::Relaxed) || state.closed {
            return false;
        }
        match product {
            Product::Init(bytes) => {
                state.init.get_or_insert_with(|| Arc::new(bytes));
            }
            Product::Segment(segment, bytes) => {
                state.segments.insert(segment, Arc::new(bytes));
                if let Some(job) = state.job.as_mut().filter(|job| job.id == id) {
                    job.next = segment + 1;
                }
                let low = state.wanted.saturating_sub(BEHIND);
                let high = state.wanted + AHEAD + REACH + 1;
                state
                    .segments
                    .retain(|kept, _| (low..=high).contains(kept) || *kept == segment);
                self.changed.notify_all();
                while !stop.load(Ordering::Relaxed)
                    && !state.closed
                    && segment + 1 > state.wanted + AHEAD
                {
                    state = sync::wait(&self.changed, state);
                }
            }
        }
        self.changed.notify_all();
        !stop.load(Ordering::Relaxed) && !state.closed
    }
}

/// Why the job a request started failed, if it did.
fn failed(state: &VodState, mine: Option<u64>) -> Option<Error> {
    match (&state.failure, mine) {
        (Some((id, why)), Some(mine)) if *id == mine => Some(Error::Source(why.clone())),
        _ => None,
    }
}

/// A live stream's HLS segments: one job from where the stream is when
/// first asked for, keeping the last few segments. A source that isn't
/// live, only silent about how long it is, is read no faster than the
/// device plays it: a few segments past the last it asked for.
pub(crate) struct Live {
    media: Arc<Media>,
    paced: bool,
    state: Mutex<LiveState>,
    changed: Condvar,
}

#[derive(Default)]
struct LiveState {
    init: Option<Arc<Vec<u8>>>,
    segments: VecDeque<LiveSegment>,
    next_sequence: usize,
    /// One past the last segment asked for.
    wanted: usize,
    running: bool,
    ended: bool,
    failure: Option<String>,
    closed: bool,
    stop: Arc<AtomicBool>,
}

pub(crate) struct LiveSegment {
    pub sequence: usize,
    pub duration: f64,
    pub bytes: Arc<Vec<u8>>,
}

/// A live playlist's segments: their sequence numbers and lengths.
pub(crate) struct LiveWindow {
    pub segments: Vec<(usize, f64)>,
    pub ended: bool,
}

impl Live {
    pub fn new(media: Arc<Media>) -> Arc<Self> {
        Arc::new(Self {
            paced: !media.source.is_live(),
            media,
            state: Mutex::default(),
            changed: Condvar::new(),
        })
    }

    /// The segments to list, once there are a couple to start on (a paced
    /// job makes `AHEAD` before any is asked for).
    pub fn window(self: &Arc<Self>) -> Result<LiveWindow, Error> {
        let deadline = Instant::now() + WAIT;
        let mut state = lock(&self.state);
        self.run_if_idle(&mut state);
        loop {
            if state.segments.len() >= 2 || state.ended {
                if state.segments.is_empty()
                    && let Some(why) = &state.failure
                {
                    return Err(Error::Source(why.clone()));
                }
                let first = state.segments.len().saturating_sub(LIVE_WINDOW);
                return Ok(LiveWindow {
                    segments: state
                        .segments
                        .iter()
                        .skip(first)
                        .map(|segment| (segment.sequence, segment.duration))
                        .collect(),
                    ended: state.ended,
                });
            }
            state = self.wait(state, deadline)?;
        }
    }

    pub fn init(self: &Arc<Self>) -> Result<Arc<Vec<u8>>, Error> {
        let deadline = Instant::now() + WAIT;
        let mut state = lock(&self.state);
        self.run_if_idle(&mut state);
        loop {
            if let Some(init) = &state.init {
                return Ok(Arc::clone(init));
            }
            if state.ended {
                return Err(Error::Source(
                    state
                        .failure
                        .clone()
                        .unwrap_or_else(|| "the stream ended".into()),
                ));
            }
            state = self.wait(state, deadline)?;
        }
    }

    pub fn segment(self: &Arc<Self>, sequence: usize) -> Result<Arc<Vec<u8>>, Error> {
        let deadline = Instant::now() + WAIT;
        let mut state = lock(&self.state);
        self.run_if_idle(&mut state);
        if sequence >= state.wanted && sequence <= state.next_sequence + REACH {
            state.wanted = sequence + 1;
            self.changed.notify_all();
        }
        loop {
            if let Some(segment) = state.segments.iter().find(|s| s.sequence == sequence) {
                return Ok(Arc::clone(&segment.bytes));
            }
            if sequence < state.next_sequence
                || sequence > state.next_sequence + REACH
                || state.ended
            {
                return Err(Error::Source(format!("segment {sequence} isn't there")));
            }
            state = self.wait(state, deadline)?;
        }
    }

    pub fn close(&self) {
        let mut state = lock(&self.state);
        state.closed = true;
        state.ended = true;
        state.stop.store(true, Ordering::Relaxed);
        state.segments.clear();
        self.changed.notify_all();
    }

    fn wait<'a>(
        &self,
        state: MutexGuard<'a, LiveState>,
        deadline: Instant,
    ) -> Result<MutexGuard<'a, LiveState>, Error> {
        wait_until(&self.changed, state, deadline, "the stream took too long")
    }

    fn run_if_idle(self: &Arc<Self>, state: &mut LiveState) {
        if state.running || state.ended {
            return;
        }
        state.running = true;
        let live = Arc::clone(self);
        let stop = Arc::clone(&state.stop);
        let spawned = thread::Builder::new()
            .name("stream-server live".into())
            .spawn(move || live.run(&stop));
        if let Err(err) = spawned {
            state.running = false;
            state.ended = true;
            state.failure = Some(format!("can't start the stream: {err}"));
        }
    }

    fn run(&self, stop: &Arc<AtomicBool>) {
        let result = make_live(&self.media, stop, &mut |product| self.store(product));
        let mut state = lock(&self.state);
        if let Err(err) = result
            && !stop.load(Ordering::Relaxed)
        {
            log::warn!("the live stream failed: {err}");
            state.failure = Some(err.to_string());
        }
        state.running = false;
        state.ended = true;
        self.changed.notify_all();
    }

    fn store(&self, product: LiveProduct) -> bool {
        let mut state = lock(&self.state);
        match product {
            LiveProduct::Init(bytes) => state.init = Some(Arc::new(bytes)),
            LiveProduct::Segment { duration, bytes } => {
                let sequence = state.next_sequence;
                state.next_sequence += 1;
                state.segments.push_back(LiveSegment {
                    sequence,
                    duration,
                    bytes: Arc::new(bytes),
                });
                while state.segments.len() > LIVE_KEPT {
                    state.segments.pop_front();
                }
                self.changed.notify_all();
                while self.paced && !state.closed && state.next_sequence >= state.wanted + AHEAD {
                    state = sync::wait(&self.changed, state);
                }
            }
        }
        self.changed.notify_all();
        !state.closed
    }
}

enum LiveProduct {
    Init(Vec<u8>),
    Segment { duration: f64, bytes: Vec<u8> },
}

/// Make a live stream's segments, cut at the leading stream's first
/// keyframe a segment's length after the last cut.
fn make_live(
    media: &Media,
    stop: &Arc<AtomicBool>,
    deliver: &mut dyn FnMut(LiveProduct) -> bool,
) -> Result<(), Error> {
    let keys = Keys::Every {
        period: media.segment,
        next: None,
    };
    let mut pipeline =
        Pipeline::open(media, Start::Beginning, keys, Domain::Pts, Arc::clone(stop))?;
    let mut start: Option<f64> = None;
    let mut latest = f64::NEG_INFINITY;
    let mut held: Vec<Out> = Vec::new();
    let read = read_through(&mut pipeline, stop, Broken::Ends, &mut |pipeline, out| {
        let leader = out.output == 0;
        latest = latest.max(out.time);
        if leader && start.is_none() {
            start = Some(out.time);
        }
        if let Some(begun) = start
            && leader
            && out.key
            && out.time - begun >= media.segment - EPSILON
        {
            let (before, after): (Vec<Out>, Vec<Out>) = held
                .drain(..)
                .partition(|follower| follower.time < out.time);
            for follower in before {
                pipeline.write(follower)?;
            }
            let segment = LiveProduct::Segment {
                duration: out.time - begun,
                bytes: pipeline.muxer.cut()?,
            };
            if !hand_over(&mut pipeline.muxer, segment, LiveProduct::Init, deliver) {
                return Ok(false);
            }
            start = Some(out.time);
            for follower in after {
                pipeline.write(follower)?;
            }
        } else if !leader && start.is_some_and(|begun| out.time >= begun + media.segment - EPSILON)
        {
            held.push(out);
            return Ok(true);
        }
        pipeline.write(out)?;
        Ok(true)
    })?;
    if !read {
        return Ok(());
    }
    for follower in held {
        pipeline.write(follower)?;
    }
    let bytes = pipeline.muxer.finish()?;
    let duration = start.map_or(0.0, |begun| (latest - begun).max(0.0));
    let last = LiveProduct::Segment { duration, bytes };
    hand_over(&mut pipeline.muxer, last, LiveProduct::Init, deliver);
    Ok(())
}

/// Hand over what was cut, after MP4's init segment if cutting it wrote
/// that.
fn hand_over<P>(
    muxer: &mut Muxer,
    cut: P,
    init: fn(Vec<u8>) -> P,
    deliver: &mut dyn FnMut(P) -> bool,
) -> bool {
    if let Some(bytes) = muxer.take_init()
        && !deliver(init(bytes))
    {
        return false;
    }
    deliver(cut)
}

/// A progressive MPEG-TS stream of the media from `start` seconds, sent
/// in chunks as it is made; it stops when the receiver is dropped or
/// `stop` is set.
pub(crate) fn stream(media: Arc<Media>, start: f64, stop: Arc<AtomicBool>) -> Receiver<Vec<u8>> {
    let (sender, receiver) = sync_channel(8);
    let spawned = thread::Builder::new()
        .name("stream-server stream".into())
        .spawn(move || {
            if let Err(err) = make_stream(&media, start, &stop, &sender)
                && !stop.load(Ordering::Relaxed)
            {
                log::warn!("streaming from {start:.1} s failed: {err}");
            }
        });
    if let Err(err) = spawned {
        log::warn!("can't start a stream: {err}");
    }
    receiver
}

fn make_stream(
    media: &Media,
    start: f64,
    stop: &Arc<AtomicBool>,
    sender: &SyncSender<Vec<u8>>,
) -> Result<(), Error> {
    let from = if start > media.info.start + EPSILON {
        Start::Time(start)
    } else {
        Start::Beginning
    };
    let keys = Keys::Every {
        period: media.segment,
        next: None,
    };
    let mut pipeline = Pipeline::open(media, from, keys, Domain::Pts, Arc::clone(stop))?;
    let mut written = 0;
    let mut outs = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let more = pipeline.step(&mut outs, Broken::Ends)?;
        for out in outs.drain(..) {
            pipeline.write(out)?;
            written += 1;
        }
        if written >= STREAM_BATCH || !more {
            written = 0;
            let bytes = if more {
                pipeline.muxer.take()?
            } else {
                pipeline.muxer.finish()?
            };
            if !bytes.is_empty() && sender.send(bytes).is_err() {
                // The device hung up.
                return Ok(());
            }
        }
        if !more {
            return Ok(());
        }
    }
}

#[cfg(test)]
#[path = "tests/job.rs"]
mod tests;
