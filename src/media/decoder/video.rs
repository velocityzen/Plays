//! Decoding video: packets into frames, on the GPU when it can, made into
//! pictures to show.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use ffmpeg::{Discard, Packet, codec, frame};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;

#[cfg(not(target_os = "macos"))]
use super::convert::gpu_conversion;
use super::convert::{Scaler, convert, output_size};
use super::demux::packet_time;
use super::{Session, StreamInfo, VideoFrame, codec_name};
use crate::media::hwaccel;
use crate::media::orientation::Orientation;
use crate::media::picture::Picture;
#[cfg(target_os = "macos")]
use crate::media::videotoolbox;
#[cfg(not(target_os = "macos"))]
use crate::media::yuv::YuvConverter;

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

pub(super) struct VideoDecoder {
    pub(super) index: usize,
    pub(super) decoder: ffmpeg::decoder::Video,
    /// Kept to reopen the decoder in software if the hardware one fails.
    parameters: codec::Parameters,
    /// Which way up the pictures go, from the stream's display matrix.
    pub(super) orientation: Orientation,
    pub(super) time_base: f64,
    pub(super) frame_duration: f64,
    max_short_side: Option<u32>,
    scaler: Option<Scaler>,
    decoded: frame::Video,
    /// Hardware frames copied back from the GPU.
    downloaded: frame::Video,
    /// The hardware decoding API attached to the decoder, if any.
    hardware: Option<&'static str>,
    /// Whether the last frame came from the hardware decoder, as last reported.
    reported: Option<bool>,
    /// Turns hardware frames into surfaces, created on first use; `Err` once
    /// that failed, after which frames are copied to memory instead.
    #[cfg(target_os = "macos")]
    surfaces: Option<Result<videotoolbox::Converter, ()>>,
    /// Hands frames to gpui as their planes, for the GPU to convert.
    #[cfg(not(target_os = "macos"))]
    planes: YuvConverter,
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

/// Open a decoder for the stream, on the GPU when asked and possible.
pub(in crate::media) fn open_video(
    parameters: &codec::Parameters,
    hardware: bool,
) -> Result<(ffmpeg::decoder::Video, Option<&'static str>)> {
    let mut context = codec::context::Context::from_parameters(parameters.clone())?;
    let codec = ffmpeg::decoder::find(context.id())
        .ok_or_else(|| anyhow!("there is no decoder for {}", codec_name(parameters.id())))?;
    // Several files decode at once, so a few threads each is plenty and
    // keeps memory down (FFmpeg's default is one per core per decoder).
    let threads = thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 6));
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
        (Ok(decoder), _) => Ok((decoder, api)),
        (Err(err), Some(api)) => {
            log::info!("{api} couldn't open this video ({err}); decoding in software");
            open_video(parameters, false)
        }
        (Err(err), None) => Err(err).context("opening the video decoder"),
    }
}

impl VideoDecoder {
    pub(super) fn new(
        stream: &StreamInfo,
        max_short_side: Option<u32>,
        hardware: bool,
    ) -> Result<Self> {
        let parameters = stream.parameters.clone();
        let (decoder, hardware) = open_video(&parameters, hardware)?;
        if decoder.width() == 0 || decoder.height() == 0 {
            bail!("video stream has no dimensions");
        }
        let frame_duration = stream.frame_rate.map_or(1.0 / 25.0, |fps| 1.0 / fps);
        Ok(Self {
            index: stream.index,
            decoder,
            orientation: Orientation::of(&parameters),
            parameters,
            time_base: f64::from(stream.time_base),
            frame_duration,
            max_short_side,
            scaler: None,
            decoded: frame::Video::empty(),
            downloaded: frame::Video::empty(),
            hardware,
            reported: None,
            #[cfg(target_os = "macos")]
            surfaces: None,
            #[cfg(not(target_os = "macos"))]
            planes: YuvConverter::default(),
            errors: 0,
            next_pts: 0.0,
            position: None,
            shown: None,
            reached: false,
            finished: false,
        })
    }

    pub(super) fn codec_name(&self) -> String {
        codec_name(self.decoder.id())
    }

    pub(super) fn display_size(&self) -> (u32, u32) {
        output_size(
            self.decoder.width(),
            self.decoder.height(),
            self.decoder.aspect_ratio(),
            self.max_short_side,
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
        if let Err(err) = self.decoder.send_packet(packet) {
            log::debug!("video decode error: {err}");
            self.failed(session);
        }
        self.drain(session);
    }

    pub(super) fn finish(&mut self, session: &Session) {
        let _ = self.decoder.send_eof();
        self.drain(session);
        self.finished = true;
        session.shared.set_video_ended(session.serial);
    }

    /// Count a decoding error. A hardware decoder that keeps failing (a driver
    /// that accepts a stream it can't actually decode) is swapped for a
    /// software one; it picks up again at the next keyframe.
    fn failed(&mut self, session: &Session) {
        let Some(api) = self.hardware else {
            return;
        };
        self.errors += 1;
        if self.errors < HARDWARE_ERROR_LIMIT {
            return;
        }
        log::warn!(
            "{}: {api} keeps failing on this video; decoding in software",
            session.name
        );
        match open_video(&self.parameters, false) {
            Ok((decoder, _)) => {
                self.decoder = decoder;
                self.hardware = None;
                self.errors = 0;
            }
            Err(err) => log::warn!("{}: {err:#}", session.name),
        }
    }

    /// Note in the media info whether frames come from the GPU.
    fn report(&mut self, hardware: bool, session: &Session) {
        if self.reported == Some(hardware) {
            return;
        }
        self.reported = Some(hardware);
        let api = self.hardware.filter(|_| hardware);
        match (api, self.hardware) {
            (Some(api), _) => log::info!("{}: decoding video with {api}", session.name),
            (None, Some(api)) => log::info!(
                "{}: {api} can't decode this video; decoding in software",
                session.name
            ),
            (None, None) => {}
        }
        session.shared.update_info(|info| info.hardware = api);
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
            if let Some(picture) = self.picture(on_gpu, session) {
                session.shared.push_frame(VideoFrame {
                    pts,
                    timestamp,
                    serial: session.serial,
                    picture,
                });
            }
        }
    }

    /// Make the decoded frame drawable.
    fn picture(&mut self, on_gpu: bool, session: &Session) -> Option<Picture> {
        #[cfg(target_os = "macos")]
        if on_gpu && let Some(surface) = self.surface(session) {
            return Some(Picture::Surface(surface));
        }
        // Elsewhere, frames decoded on the GPU are copied back to draw.
        #[cfg(not(target_os = "macos"))]
        let _ = session;
        let frame = if on_gpu {
            if let Err(err) = hwaccel::download(&self.decoded, &mut self.downloaded) {
                log::warn!("{err:#}");
                return None;
            }
            &self.downloaded
        } else {
            &self.decoded
        };
        #[cfg(not(target_os = "macos"))]
        if gpu_conversion() {
            match self.planes.convert(frame, self.max_short_side) {
                Some(Ok(planes)) => return Some(Picture::Yuv(planes)),
                Some(Err(err)) => log::warn!("handing the frame to the GPU failed: {err:#}"),
                None => {}
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
    fn surface(&mut self, session: &Session) -> Option<videotoolbox::Surface> {
        let (width, height) = output_size(
            self.decoded.width(),
            self.decoded.height(),
            self.decoded.aspect_ratio(),
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
        match converter.convert(&self.decoded, width, height) {
            Ok(surface) => Some(surface),
            Err(err) => {
                log::warn!("{}: {err:#}; copying frames from the GPU", session.name);
                self.surfaces = Some(Err(()));
                None
            }
        }
    }

    /// Decode a single picture, used for embedded cover art.
    pub(super) fn decode_still(&mut self, packet: &Packet) -> Option<Arc<RenderImage>> {
        self.decoder.send_packet(packet).ok()?;
        let _ = self.decoder.send_eof();
        self.decoder.receive_frame(&mut self.decoded).ok()?;
        convert(&mut self.scaler, self.max_short_side, &self.decoded)
            .inspect_err(|err| log::warn!("cover art conversion failed: {err:#}"))
            .ok()
    }
}
