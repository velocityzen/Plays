//! Decoding video: packets into frames, on the GPU when it can, made into
//! pictures to show.

use std::collections::VecDeque;
use std::num::NonZero;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use ffmpeg::{Discard, Packet, Rational, codec, ffi, frame};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;

use super::base_layer::BaseLayer;
#[cfg(not(target_os = "macos"))]
use super::convert::gpu_conversion;
use super::convert::{Scaler, convert, output_size, set_aspect};
use super::demux::packet_time;
use super::layers::{Found, Layers};
use super::{Session, StreamInfo, VideoFrame, codec_name};
use crate::media::enhancement::Composer;
use crate::media::hwaccel;
use crate::media::orientation::Orientation;
use crate::media::picture::Picture;
#[cfg(target_os = "macos")]
use crate::media::videotoolbox;
#[cfg(not(target_os = "macos"))]
use crate::media::yuv::YuvConverter;
#[cfg(target_os = "macos")]
use crate::media::{dovi, hdr};

/// Decoded video kept ahead of the playhead, by memory.
const VIDEO_QUEUE_BYTES: usize = 32 * 1024 * 1024;
const VIDEO_QUEUE_MIN_FRAMES: usize = 3;
const VIDEO_QUEUE_MAX_FRAMES: usize = 16;
/// A scrub this far ahead of the frame decoded last, in seconds, decodes on
/// from there rather than seeking again.
const SCRUB_AHEAD: f64 = 2.0;
/// While a scrub decodes its way to the frame asked for, one on the way
/// shows this often, so the picture follows the slider.
const SCRUB_SHOWS_EVERY: Duration = Duration::from_millis(40);
/// Consecutive decoding errors after which a hardware decoder is replaced by
/// a software one.
const HARDWARE_ERROR_LIMIT: u32 = 8;
/// Pixels a software decoding thread is given at most, past six threads:
/// a sixth of 4K.
const PIXELS_PER_THREAD: usize = 3840 * 2160 / 6;
/// Frames waiting for their enhancement layer at most, past which the
/// oldest shows without it: more than its decoder's threads hold back.
const MOST_WAITING: usize = 12;

pub(super) struct VideoDecoder {
    pub(super) index: usize,
    /// The shape of its pixels, where its container gives it.
    aspect: Option<Rational>,
    pub(super) decoder: ffmpeg::decoder::Video,
    /// Kept to reopen the decoder in software if the hardware one fails.
    parameters: codec::Parameters,
    /// Which way up the pictures go, from the stream's display matrix.
    pub(super) orientation: Orientation,
    pub(super) time_base: f64,
    pub(super) frame_duration: f64,
    pictures: Pictures,
    decoded: frame::Video,
    /// The hardware the decoder decodes on, if any.
    hardware: Option<Hardware>,
    /// Keeps the packets of HEVC of more than one layer to the base layer,
    /// for the hardware decoder, which takes that alone.
    base: Option<BaseLayer>,
    /// Whether the last frame came from the hardware decoder, as last reported.
    reported: Option<bool>,
    /// Dolby Vision profile 7's enhancement layer, decoded beside it.
    layers: Option<Layers>,
    /// Frames to show once their enhancement layer is decoded, in order.
    waiting: VecDeque<Waiting>,
    errors: u32,
    next_pts: f64,
    /// The time of the frame out of the decoder last since it was flushed.
    position: Option<f64>,
    /// In a scrub: when a frame on the way last went to the queue, and
    /// whether the frame asked for has.
    shown: Option<Instant>,
    pub(super) reached: bool,
    pub(super) finished: bool,
}

/// A frame waiting for its enhancement layer, with what it shows by.
struct Waiting {
    frame: frame::Video,
    pts: f64,
    timestamp: Option<i64>,
    on_gpu: bool,
}

/// Makes decoded frames drawable.
struct Pictures {
    show: Show,
    /// Hardware frames copied back from the GPU.
    downloaded: frame::Video,
    /// The last frame composed of Dolby Vision's two layers.
    composed: frame::Video,
    /// Whether composing has failed, said once.
    failed: bool,
}

/// Makes frames in memory, or on the GPU on macOS, drawable.
struct Show {
    max_short_side: Option<u32>,
    scaler: Option<Scaler>,
    /// Turns hardware frames into surfaces, created on first use; `Err` once
    /// that failed, after which frames are copied to memory instead.
    #[cfg(target_os = "macos")]
    surfaces: Option<Result<videotoolbox::Converter, ()>>,
    /// Turns HDR frames in memory into surfaces too, to show as bright; as
    /// `surfaces`, `Err` once that failed.
    #[cfg(target_os = "macos")]
    uploads: Option<Result<videotoolbox::Converter, ()>>,
    /// Hands frames to gpui as their planes, for the GPU to convert.
    #[cfg(not(target_os = "macos"))]
    planes: YuvConverter,
}

/// The hardware a decoder decodes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::media) struct Hardware {
    pub(in crate::media) api: &'static str,
    /// Whether the decoder decodes only on it (FFmpeg's own AV1 decoder),
    /// so that its first error has it replaced, not a few.
    pub(in crate::media) only: bool,
}

/// Open a decoder for the stream, on the GPU when asked and possible.
pub(in crate::media) fn open_video(
    parameters: &codec::Parameters,
    hardware: bool,
) -> Result<(ffmpeg::decoder::Video, Option<Hardware>)> {
    let mut context = codec::context::Context::from_parameters(parameters.clone())?;
    let default = ffmpeg::decoder::find(context.id())
        .ok_or_else(|| anyhow!("there is no decoder for {}", codec_name(parameters.id())))?;
    let instead = hardware.then(|| hwaccel::instead_of(&default)).flatten();
    let only = instead.is_some();
    let codec = instead.unwrap_or(default);
    // Several files decode at once, so a few threads each is plenty and
    // keeps memory down (FFmpeg's default is one per core per decoder);
    // a picture bigger than 4K takes more, up to every core, to keep up.
    // SAFETY: reading the parameters' size.
    let pixels = unsafe {
        let parameters = parameters.as_ptr();
        usize::try_from((*parameters).width).unwrap_or(0)
            * usize::try_from((*parameters).height).unwrap_or(0)
    };
    let threads = threads(
        thread::available_parallelism().map_or(4, NonZero::get),
        pixels,
    );
    // `safe` only exists with older FFmpeg versions, hence the struct update.
    #[allow(clippy::needless_update)]
    let threading = codec::threading::Config {
        kind: codec::threading::Type::Frame,
        count: threads,
        ..Default::default()
    };
    context.set_threading(threading);
    let api = if hardware {
        hwaccel::attach(&mut context, &codec)
    } else {
        None
    };
    match (
        context.decoder().open_as(codec).and_then(|d| d.video()),
        api,
    ) {
        (Ok(decoder), _) => Ok((decoder, api.map(|api| Hardware { api, only }))),
        (Err(err), Some(api)) => {
            log::info!("{api} couldn't open this video ({err}); decoding in software");
            open_video(parameters, false)
        }
        (Err(err), None) => Err(err).context("opening the video decoder"),
    }
}

/// Threads to decode a picture of `pixels` with, of `cores`: two to six,
/// or more for one bigger than 4K.
fn threads(cores: usize, pixels: usize) -> usize {
    let few = cores.clamp(2, 6);
    pixels
        .div_ceil(PIXELS_PER_THREAD)
        .clamp(few, cores.max(few))
}

impl VideoDecoder {
    pub(super) fn new(
        stream: &StreamInfo,
        max_short_side: Option<u32>,
        hardware: bool,
    ) -> Result<Self> {
        let parameters = stream.parameters.clone();
        let base = hardware.then(|| BaseLayer::of(&parameters)).flatten();
        let (decoder, hardware) = open_video(
            base.as_ref()
                .map_or(&parameters, |(_, parameters)| parameters),
            hardware,
        )?;
        let base = base.filter(|_| hardware.is_some()).map(|(base, _)| base);
        if decoder.width() == 0 || decoder.height() == 0 {
            bail!("video stream has no dimensions");
        }
        let frame_duration = stream.frame_rate.map_or(1.0 / 25.0, |fps| 1.0 / fps);
        Ok(Self {
            index: stream.index,
            aspect: stream.aspect,
            decoder,
            orientation: Orientation::of(&parameters),
            parameters,
            time_base: f64::from(stream.time_base),
            frame_duration,
            pictures: Pictures {
                show: Show {
                    max_short_side,
                    scaler: None,
                    #[cfg(target_os = "macos")]
                    surfaces: None,
                    #[cfg(target_os = "macos")]
                    uploads: None,
                    #[cfg(not(target_os = "macos"))]
                    planes: YuvConverter::default(),
                },
                downloaded: frame::Video::empty(),
                composed: frame::Video::empty(),
                failed: false,
            },
            decoded: frame::Video::empty(),
            hardware,
            base,
            reported: None,
            layers: None,
            waiting: VecDeque::new(),
            errors: 0,
            next_pts: 0.0,
            position: None,
            shown: None,
            reached: false,
            finished: false,
        })
    }

    /// Decode Dolby Vision profile 7's enhancement layer with it, if
    /// `stream` has one among `streams`.
    pub(super) fn with_layers(mut self, stream: &StreamInfo, streams: &[StreamInfo]) -> Self {
        let streams: Vec<_> = streams.iter().map(StreamInfo::as_layer).collect();
        self.layers = Layers::of(stream.as_layer(), self.frame_duration, &streams);
        self
    }

    /// The stream of its enhancement layer, when that has one of its own,
    /// whose packets come with its own.
    pub(super) fn layer_stream(&self) -> Option<usize> {
        self.layers.as_ref()?.stream
    }

    pub(super) fn codec_name(&self) -> String {
        codec_name(self.decoder.id())
    }

    pub(super) fn display_size(&self) -> (u32, u32) {
        output_size(
            self.decoder.width(),
            self.decoder.height(),
            self.aspect.unwrap_or_else(|| self.decoder.aspect_ratio()),
            self.pictures.show.max_short_side,
        )
    }

    /// Keep roughly `VIDEO_QUEUE_BYTES` of decoded frames ready.
    pub(super) fn max_queued_frames(&self) -> usize {
        let (w, h) = self.display_size();
        let frame_bytes = (w as usize * h as usize * 4).max(1);
        (VIDEO_QUEUE_BYTES / frame_bytes).clamp(VIDEO_QUEUE_MIN_FRAMES, VIDEO_QUEUE_MAX_FRAMES)
    }

    pub(super) fn reset(&mut self) {
        self.decoder.flush();
        if let Some(layers) = &mut self.layers {
            layers.reset();
        }
        self.waiting.clear();
        self.finished = false;
        self.position = None;
        self.retarget();
    }

    /// Whether a scrub to `time` decodes on to it: it is ahead of the frame
    /// decoded last (or, before any, of where the last seek was going), and
    /// not far.
    pub(super) fn reaches(&self, time: f64, target: Option<f64>) -> bool {
        self.position
            .or(target)
            .is_some_and(|from| time >= from && time - from <= SCRUB_AHEAD)
    }

    /// Head for another frame of a scrub without seeking.
    pub(super) fn retarget(&mut self) {
        self.shown = None;
        self.reached = false;
    }

    pub(super) fn decode(&mut self, packet: &Packet, session: &Session) {
        if packet.stream() != self.index {
            // The enhancement layer's own stream.
            if let Some(layers) = &mut self.layers {
                layers.decode(packet);
            }
            self.release(session);
            return;
        }
        // Its enhancement layer first, for its frames to be there.
        if let Some(layers) = &mut self.layers
            && layers.stream.is_none()
        {
            layers.decode(packet);
        }
        // Frames other frames don't refer to aren't decoded on the way to a
        // seek's target, as they wouldn't be shown: it gets there sooner.
        let before_target = session.seek_target.is_some_and(|target| {
            packet_time(packet, self.time_base).is_some_and(|time| {
                time - session.start_offset() + 2.0 * self.frame_duration <= target
            })
        });
        self.decoder.skip_frame(if before_target {
            Discard::NonReference
        } else {
            Discard::Default
        });
        if let Err(err) = self.send(packet) {
            log::debug!("video decode error: {err}");
            // The decoder in its place starts with the packet this one
            // failed on: the first one decoded, when the GPU can't.
            if self.failed(session) {
                self.decoder.skip_frame(if before_target {
                    Discard::NonReference
                } else {
                    Discard::Default
                });
                if let Err(err) = self.send(packet) {
                    log::debug!("video decode error: {err}");
                }
            }
        }
        self.drain(session);
    }

    /// Give the decoder `packet`, or its base layer alone where it takes
    /// only that.
    fn send(&mut self, packet: &Packet) -> Result<(), ffmpeg::Error> {
        match self.base.as_ref().and_then(|base| base.packet(packet)) {
            // None of it is the base layer's: an empty packet would be an
            // error, or the end.
            Some(base) if base.size() == 0 => Ok(()),
            Some(base) => self.decoder.send_packet(&base),
            None => self.decoder.send_packet(packet),
        }
    }

    pub(super) fn finish(&mut self, session: &Session) {
        let _ = self.decoder.send_eof();
        self.drain(session);
        if let Some(layers) = &mut self.layers {
            layers.finish();
        }
        self.release(session);
        self.finished = true;
        session.shared.set_video_ended(session.serial);
    }

    /// Count a decoding error. A hardware decoder that keeps failing (a driver
    /// that accepts a stream it can't actually decode) is swapped for a
    /// software one, which picks up again at the next keyframe; one that
    /// only decodes on the GPU, at once. Says whether it was swapped.
    fn failed(&mut self, session: &Session) -> bool {
        let Some(Hardware { api, only }) = self.hardware else {
            return false;
        };
        self.errors += 1;
        if !only && self.errors < HARDWARE_ERROR_LIMIT {
            return false;
        }
        if only {
            log::info!(
                "{}: {api} can't decode this video; decoding in software",
                session.name
            );
        } else {
            log::warn!(
                "{}: {api} keeps failing on this video; decoding in software",
                session.name
            );
        }
        match open_video(&self.parameters, false) {
            Ok((decoder, _)) => {
                self.decoder = decoder;
                self.hardware = None;
                self.base = None;
                self.errors = 0;
                true
            }
            Err(err) => {
                log::warn!("{}: {err:#}", session.name);
                false
            }
        }
    }

    /// Note in the media info whether frames come from the GPU.
    fn report(&mut self, hardware: bool, session: &Session) {
        if self.reported == Some(hardware) {
            return;
        }
        self.reported = Some(hardware);
        let api = self.hardware.map(|hardware| hardware.api);
        let shown = api.filter(|_| hardware);
        match (shown, api) {
            (Some(api), _) => log::info!("{}: decoding video with {api}", session.name),
            (None, Some(api)) => log::info!(
                "{}: {api} can't decode this video; decoding in software",
                session.name
            ),
            (None, None) => {}
        }
        session.shared.update_info(|info| info.hardware = shown);
    }

    fn drain(&mut self, session: &Session) {
        loop {
            match self.decoder.receive_frame(&mut self.decoded) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => break,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                    break;
                }
                Err(err) => {
                    log::debug!("video decode error: {err}");
                    self.failed(session);
                    break;
                }
            }
            self.errors = 0;
            set_aspect(&mut self.decoded, self.aspect);
            let on_gpu = hwaccel::is_hardware_frame(&self.decoded);
            self.report(on_gpu, session);

            let timestamp = self.decoded.timestamp();
            let pts = timestamp
                .map(|ts| ts as f64 * self.time_base - session.start_offset())
                .unwrap_or(self.next_pts);
            self.next_pts = pts + self.frame_duration;
            self.position = Some(pts);

            let before_target = session
                .seek_target
                .is_some_and(|target| pts + self.frame_duration <= target);
            if session.scrub {
                // The first frame on the way shows at once, then one now and
                // then; the one asked for ends it.
                if self.reached {
                    continue;
                }
                if before_target {
                    if self
                        .shown
                        .is_some_and(|shown| shown.elapsed() < SCRUB_SHOWS_EVERY)
                    {
                        continue;
                    }
                    self.shown = Some(Instant::now());
                } else {
                    self.reached = true;
                }
            } else {
                // Skip conversion for frames that end before the seek target.
                if before_target {
                    continue;
                }
                // Skip frames whose time has already passed while something
                // else is still waiting to be shown (the UI isn't keeping up,
                // e.g. the window is hidden). With nothing queued, show it
                // anyway so a decoder that is slower than real time still puts
                // pictures on screen.
                let late = pts + self.frame_duration < session.shared.clock().now();
                if late && session.shared.queued_frames() > 0 {
                    continue;
                }
            }
            if self.layers.is_some()
                && let Some(frame) = reference(&self.decoded)
            {
                self.waiting.push_back(Waiting {
                    frame,
                    pts,
                    timestamp,
                    on_gpu,
                });
                self.release(session);
            } else if let Some(picture) = self.pictures.picture(&self.decoded, on_gpu, session) {
                session.shared.push_frame(VideoFrame {
                    pts,
                    timestamp,
                    serial: session.serial,
                    picture,
                });
            }
        }
    }

    /// Show the frames waiting whose enhancement layer has been decoded,
    /// composed with it, and those whose won't be, without. A scrub
    /// doesn't wait.
    fn release(&mut self, session: &Session) {
        while let Some(front) = self.waiting.front() {
            let found = match (&mut self.layers, front.timestamp) {
                (Some(layers), Some(timestamp)) => layers.take(timestamp),
                _ => Found::Missing,
            };
            let enhancement = match found {
                Found::Later if !session.scrub && self.waiting.len() < MOST_WAITING => break,
                Found::Frame(frame) => Some(frame),
                Found::Later | Found::Missing => None,
            };
            let Some(waiting) = self.waiting.pop_front() else {
                break;
            };
            let (frame, on_gpu) = (&waiting.frame, waiting.on_gpu);
            let picture = enhancement
                .and_then(|layer| self.pictures.composed(frame, on_gpu, &layer, session))
                .or_else(|| self.pictures.picture(frame, on_gpu, session));
            if let Some(picture) = picture {
                session.shared.push_frame(VideoFrame {
                    pts: waiting.pts,
                    timestamp: waiting.timestamp,
                    serial: session.serial,
                    picture,
                });
            }
        }
    }

    /// Decode a single picture, used for embedded cover art.
    pub(super) fn decode_still(&mut self, packet: &Packet) -> Option<Arc<RenderImage>> {
        self.decoder.send_packet(packet).ok()?;
        let _ = self.decoder.send_eof();
        self.decoder.receive_frame(&mut self.decoded).ok()?;
        let show = &mut self.pictures.show;
        convert(&mut show.scaler, show.max_short_side, &self.decoded)
            .inspect_err(|err| log::warn!("cover art conversion failed: {err:#}"))
            .ok()
    }
}

/// Another reference to `frame`'s pictures, which aren't copied.
fn reference(frame: &frame::Video) -> Option<frame::Video> {
    let mut copy = frame::Video::empty();
    // SAFETY: both frames are valid, and the new one is blank.
    let referenced = unsafe { ffi::av_frame_ref(copy.as_mut_ptr(), frame.as_ptr()) };
    (referenced >= 0).then_some(copy)
}

impl Pictures {
    /// Make a decoded frame drawable.
    fn picture(
        &mut self,
        frame: &frame::Video,
        on_gpu: bool,
        session: &Session,
    ) -> Option<Picture> {
        #[cfg(target_os = "macos")]
        if on_gpu && let Some(surface) = self.show.surface(frame, session) {
            return Some(Picture::Surface(surface));
        }
        // Elsewhere, frames decoded on the GPU are copied back to draw.
        let frame = if on_gpu {
            if let Err(err) = hwaccel::download(frame, &mut self.downloaded) {
                log::warn!("{err:#}");
                return None;
            }
            &self.downloaded
        } else {
            frame
        };
        self.show.in_memory(frame, session)
    }

    /// Make a picture of Dolby Vision's two layers composed; `None` if they
    /// don't compose.
    fn composed(
        &mut self,
        base: &frame::Video,
        on_gpu: bool,
        enhancement: &frame::Video,
        session: &Session,
    ) -> Option<Picture> {
        let base = if on_gpu {
            hwaccel::download(base, &mut self.downloaded)
                .inspect_err(|err| log::warn!("{err:#}"))
                .ok()?;
            &self.downloaded
        } else {
            base
        };
        // The RPU comes with the base layer in one track, with the
        // enhancement layer in two.
        let composer = Composer::of(base).or_else(|| Composer::of(enhancement))?;
        if let Err(err) = composer.compose(base, enhancement, &mut self.composed) {
            if !std::mem::replace(&mut self.failed, true) {
                log::warn!("{}: {err:#}; showing the base layer", session.name);
            }
            return None;
        }
        self.show.in_memory(&self.composed, session)
    }
}

impl Show {
    /// Make a frame in memory drawable.
    fn in_memory(&mut self, frame: &frame::Video, session: &Session) -> Option<Picture> {
        #[cfg(target_os = "macos")]
        if let Some(surface) = upload(&mut self.uploads, frame, self.max_short_side, session) {
            return Some(Picture::Surface(surface));
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = session;
            if gpu_conversion() {
                match self.planes.convert(frame, self.max_short_side) {
                    Some(Ok(planes)) => return Some(Picture::Yuv(planes)),
                    Some(Err(err)) => log::warn!("handing the frame to the GPU failed: {err:#}"),
                    None => {}
                }
            }
        }
        convert(&mut self.scaler, self.max_short_side, frame)
            .inspect_err(|err| log::warn!("video conversion failed: {err:#}"))
            .ok()
            .map(Picture::Image)
    }

    /// Convert a hardware frame without it leaving the GPU. `None` if that
    /// isn't possible, in which case it never is again for this video.
    #[cfg(target_os = "macos")]
    fn surface(
        &mut self,
        frame: &frame::Video,
        session: &Session,
    ) -> Option<videotoolbox::Surface> {
        let (width, height) = output_size(
            frame.width(),
            frame.height(),
            frame.aspect_ratio(),
            self.max_short_side,
        );
        let converter = self
            .surfaces
            .get_or_insert_with(|| {
                videotoolbox::Converter::new().map_err(|err| {
                    log::warn!("{}: {err:#}; copying frames from the GPU", session.name)
                })
            })
            .as_mut()
            .ok()?;
        match converter.convert(frame, width, height) {
            Ok(surface) => Some(surface),
            Err(err) => {
                log::warn!("{}: {err:#}; copying frames from the GPU", session.name);
                self.surfaces = Some(Err(()));
                None
            }
        }
    }
}

/// An HDR frame in memory as a surface, to show as bright as one decoded
/// on the GPU; `None` for SDR, or if that isn't possible, in which case it
/// never is again for this video.
#[cfg(target_os = "macos")]
fn upload(
    uploads: &mut Option<Result<videotoolbox::Converter, ()>>,
    frame: &frame::Video,
    max_short_side: Option<u32>,
    session: &Session,
) -> Option<videotoolbox::Surface> {
    if hdr::Transfer::of(frame).is_none() && dovi::reshaping(frame).is_none() {
        return None;
    }
    let converter = uploads
        .get_or_insert_with(|| {
            videotoolbox::Converter::new()
                .map_err(|err| log::warn!("{}: {err:#}; tone mapping HDR on the CPU", session.name))
        })
        .as_mut()
        .ok()?;
    let (width, height) = output_size(
        frame.width(),
        frame.height(),
        frame.aspect_ratio(),
        max_short_side,
    );
    match converter.upload(frame, width, height) {
        Ok(surface) => Some(surface),
        Err(err) => {
            log::warn!("{}: {err:#}; tone mapping HDR on the CPU", session.name);
            *uploads = Some(Err(()));
            None
        }
    }
}

#[cfg(test)]
#[path = "tests/video.rs"]
mod tests;
