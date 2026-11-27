//! Dolby Vision profile 7's enhancement layer (`media::enhancement`),
//! decoded beside the base layer and paired with its frames by time. In
//! the base layer's own track (Blu-ray remuxes), its NAL units come in the
//! same packets, and FFmpeg's `dovi_split` takes them out; in a track of
//! its own (a Blu-ray's transport stream, MP4), its packets come in the
//! video's queue, in the base layer's time base (`demux`). Screenshots
//! decode it with theirs (`still`).

use std::collections::VecDeque;
use std::ptr;

use anyhow::{Result, ensure};
use ffmpeg::packet::{Mut as _, Ref as _};
use ffmpeg::{Packet, Rational, codec, ffi, frame};
use ffmpeg_next as ffmpeg;

use super::video::open_video;
use ffmpeg_safe::DolbyVision;

/// Enhancement layer frames kept ahead of the base layer's, at most.
const MOST_AHEAD: usize = 32;

pub(in crate::media) struct Layers {
    /// The enhancement layer's own stream, when it isn't in the base
    /// layer's.
    pub(in crate::media) stream: Option<usize>,
    split: Option<Split>,
    decoder: ffmpeg::decoder::Video,
    decoded: frame::Video,
    /// Decoded, oldest first.
    frames: VecDeque<frame::Video>,
    /// How far apart two frames can be and be of one picture, in the base
    /// layer's time base: half a frame.
    tolerance: i64,
    finished: bool,
}

/// What finding a video's enhancement layer takes of a stream.
#[derive(Clone, Copy)]
pub(in crate::media) struct Stream<'a> {
    pub index: usize,
    pub parameters: &'a codec::Parameters,
    pub time_base: Rational,
}

/// What became of the enhancement layer of a picture.
pub(in crate::media) enum Found {
    Frame(frame::Video),
    /// Not decoded yet.
    Later,
    /// Never will be.
    Missing,
}

impl Layers {
    /// The enhancement layer of the video `base`, whose frames last
    /// `frame_duration` seconds, among `streams`, if it has one that can be
    /// decoded.
    pub(in crate::media) fn of(
        base: Stream,
        frame_duration: f64,
        streams: &[Stream],
    ) -> Option<Self> {
        Self::open(base, frame_duration, streams)
            .inspect_err(|err| {
                log::warn!("Dolby Vision's enhancement layer: {err:#}; showing its base layer");
            })
            .ok()
            .flatten()
    }

    fn open(base: Stream, frame_duration: f64, streams: &[Stream]) -> Result<Option<Self>> {
        if base.parameters.id() != codec::Id::HEVC {
            return Ok(None);
        }
        let (stream, split, parameters) = match DolbyVision::of(base.parameters) {
            Some(both) if both.base && both.enhancement => {
                let split = Split::new(base.parameters, base.time_base)?;
                let parameters = split.parameters()?;
                (None, Some(split), parameters)
            }
            _ => {
                let Some(layer) = streams.iter().find(|s| {
                    s.index != base.index
                        && s.parameters.id() == codec::Id::HEVC
                        && DolbyVision::of(s.parameters).is_some_and(DolbyVision::is_layer_only)
                }) else {
                    return Ok(None);
                };
                (Some(layer.index), None, layer.parameters.clone())
            }
        };
        let (decoder, _) = open_video(&parameters, false)?;
        let tolerance = (frame_duration / 2.0 / f64::from(base.time_base)).max(1.0) as i64;
        log::info!(
            "decoding Dolby Vision's enhancement layer, {}",
            if stream.is_some() {
                "a track of its own"
            } else {
                "split out of the video's track"
            }
        );
        Ok(Some(Self {
            stream,
            split,
            decoder,
            decoded: frame::Video::empty(),
            frames: VecDeque::new(),
            tolerance,
            finished: false,
        }))
    }

    /// Decode what `packet` has of the enhancement layer: a packet of its
    /// own stream, or of the base layer's, to split it out of.
    pub(in crate::media) fn decode(&mut self, packet: &Packet) {
        match &mut self.split {
            Some(split) => {
                for packet in split.run(packet) {
                    self.send(&packet);
                }
            }
            None => self.send(packet),
        }
    }

    fn send(&mut self, packet: &Packet) {
        if let Err(err) = self.decoder.send_packet(packet) {
            log::debug!("enhancement layer decode error: {err}");
        }
        self.drain();
    }

    fn drain(&mut self) {
        while self.decoder.receive_frame(&mut self.decoded).is_ok() {
            let frame = std::mem::replace(&mut self.decoded, frame::Video::empty());
            self.keep(frame);
        }
    }

    fn keep(&mut self, frame: frame::Video) {
        self.frames.push_back(frame);
        if self.frames.len() > MOST_AHEAD {
            self.frames.pop_front();
        }
    }

    /// The frame of the picture at `timestamp`, in the base layer's time
    /// base. Those before it are of pictures not shown, and go.
    pub(in crate::media) fn take(&mut self, timestamp: i64) -> Found {
        while let Some(front) = self.frames.front() {
            match front.timestamp() {
                Some(time) if time > timestamp + self.tolerance => return Found::Missing,
                Some(time) if time >= timestamp - self.tolerance => {
                    return self.frames.pop_front().map_or(Found::Missing, Found::Frame);
                }
                _ => {
                    self.frames.pop_front();
                }
            }
        }
        if self.finished {
            Found::Missing
        } else {
            Found::Later
        }
    }

    /// After a seek.
    pub(in crate::media) fn reset(&mut self) {
        self.decoder.flush();
        if let Some(split) = &mut self.split {
            split.flush();
        }
        self.frames.clear();
        self.finished = false;
    }

    /// At the end: decode what's left. `dovi_split` keeps nothing back.
    pub(in crate::media) fn finish(&mut self) {
        let _ = self.decoder.send_eof();
        self.drain();
        self.finished = true;
    }
}

/// FFmpeg's `dovi_split`, keeping the enhancement layer of a track that
/// has both.
struct Split(*mut ffi::AVBSFContext);

// SAFETY: a filter's context belongs to no thread, and is only used
// through `&mut`.
unsafe impl Send for Split {}

impl Split {
    fn new(parameters: &codec::Parameters, time_base: Rational) -> Result<Self> {
        // SAFETY: the filter is FFmpeg's own, and the context it allocates
        // is owned by `split` from then on, which frees it however this
        // returns; its input parameters are allocated with it.
        unsafe {
            let filter = ffi::av_bsf_get_by_name(c"dovi_split".as_ptr());
            ensure!(!filter.is_null(), "this FFmpeg has no dovi_split");
            let mut context = ptr::null_mut();
            ensure!(
                ffi::av_bsf_alloc(filter, &mut context) >= 0 && !context.is_null(),
                "no memory for dovi_split"
            );
            let split = Self(context);
            ensure!(
                ffi::avcodec_parameters_copy((*context).par_in, parameters.as_ptr()) >= 0,
                "no memory for dovi_split's parameters"
            );
            (*context).time_base_in = time_base.into();
            ensure!(
                ffi::av_opt_set((*context).priv_data, c"mode".as_ptr(), c"el".as_ptr(), 0) >= 0,
                "dovi_split has no enhancement layer mode"
            );
            let started = ffi::av_bsf_init(context);
            ensure!(
                started >= 0,
                "dovi_split couldn't start: {}",
                ffmpeg::Error::from(started)
            );
            Ok(split)
        }
    }

    /// The enhancement layer's parameters.
    fn parameters(&self) -> Result<codec::Parameters> {
        let mut parameters = codec::Parameters::new();
        // SAFETY: the context is initialized, so its output parameters are
        // set; the copy is owned by `parameters`.
        let copied =
            unsafe { ffi::avcodec_parameters_copy(parameters.as_mut_ptr(), (*self.0).par_out) };
        ensure!(
            copied >= 0,
            "no memory for the enhancement layer's parameters"
        );
        Ok(parameters)
    }

    /// The enhancement layer's packets in `packet`: none where it has no
    /// enhancement layer, as `dovi_split` never makes an empty one.
    fn run(&mut self, packet: &Packet) -> Vec<Packet> {
        // SAFETY: the context is valid. The filter takes a new reference
        // to the packet's data, not the caller's, leaving `reference` blank;
        // the packets it gives back are owned by the `Packet`s they're
        // received into.
        unsafe {
            let mut reference = Packet::empty();
            if ffi::av_packet_ref(reference.as_mut_ptr(), packet.as_ptr()) < 0 {
                return Vec::new();
            }
            let sent = ffi::av_bsf_send_packet(self.0, reference.as_mut_ptr());
            if sent < 0 {
                log::debug!("dovi_split error: {}", ffmpeg::Error::from(sent));
                return Vec::new();
            }
            let mut packets = Vec::new();
            loop {
                let mut out = Packet::empty();
                if ffi::av_bsf_receive_packet(self.0, out.as_mut_ptr()) < 0 {
                    break;
                }
                packets.push(out);
            }
            packets
        }
    }

    fn flush(&mut self) {
        // SAFETY: the context is valid.
        unsafe { ffi::av_bsf_flush(self.0) }
    }
}

impl Drop for Split {
    fn drop(&mut self) {
        // SAFETY: the context is this one's, and is never used again.
        unsafe { ffi::av_bsf_free(&mut self.0) }
    }
}

#[cfg(test)]
#[path = "tests/layers.rs"]
mod tests;
