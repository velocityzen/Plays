//! Opening a source and reading what is in it: the container, how long it
//! is, and each stream's codec, size, bit depth, HDR and channels.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ffmpeg::codec::Id;
use ffmpeg::ffi;
use ffmpeg::format::context::Input;
use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;
use ffmpeg_safe::tag;

use crate::Error;
use crate::profile::{AudioCodec, Container, Hdr, VideoCodec};

/// How long a network read may stall before it gives up, in microseconds.
const NETWORK_TIMEOUT_MICROS: &str = "15000000";

/// Where a cast's media comes from.
#[derive(Clone, PartialEq, Debug)]
pub enum Source {
    File(std::path::PathBuf),
    /// A stream's URL; `live` when it has no end to seek in.
    Stream {
        url: String,
        live: bool,
    },
}

impl Source {
    pub(crate) fn location(&self) -> String {
        match self {
            Self::File(path) => path.to_string_lossy().into_owned(),
            Self::Stream { url, .. } => url.clone(),
        }
    }

    pub(crate) fn is_live(&self) -> bool {
        matches!(self, Self::Stream { live: true, .. })
    }

    fn is_network(&self) -> bool {
        matches!(self, Self::Stream { .. })
    }
}

/// Open the source; network reads give up once `stop` is set.
pub(crate) fn open(source: &Source, stop: Arc<AtomicBool>) -> Result<Input, Error> {
    let mut options = ffmpeg::Dictionary::new();
    if source.is_network() {
        options.set("rw_timeout", NETWORK_TIMEOUT_MICROS);
        options.set("timeout", NETWORK_TIMEOUT_MICROS);
        options.set("reconnect", "1");
        options.set("reconnect_streamed", "1");
        options.set("reconnect_delay_max", "5");
        options.set("rtsp_transport", "tcp");
    }
    ffmpeg::format::input_with_interrupt_and_dictionary(
        &source.location(),
        move || stop.load(Ordering::Relaxed),
        options,
    )
    .map_err(|err| Error::Source(format!("can't open {}: {err}", source.location())))
}

/// What a source holds.
#[derive(Clone, Debug)]
pub struct MediaInfo {
    pub container: Container,
    /// Seconds, when known.
    pub duration: Option<f64>,
    /// The first timestamp, in seconds: where the timeline starts.
    pub start: f64,
    pub bit_rate: Option<u64>,
    pub streams: Vec<StreamInfo>,
}

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub index: usize,
    pub kind: StreamKind,
    pub language: Option<String>,
    pub title: Option<String>,
    /// Marked as the one to play by default.
    pub default: bool,
}

#[derive(Clone, Debug)]
pub enum StreamKind {
    Video(VideoInfo),
    Audio(AudioInfo),
    Subtitle {
        /// Text, rather than pictures (PGS, DVD).
        text: bool,
    },
    Other,
}

#[derive(Clone, Debug)]
pub struct VideoInfo {
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub frame_rate: Option<f64>,
    pub hdr: Option<Hdr>,
    /// HDR10 or HLG, as a player without Dolby Vision shows the picture: as
    /// `hdr` is, or Dolby Vision's base layer's, by its colors. None for
    /// SDR, and for a picture in IPT (Dolby Vision's profiles 5 and 20),
    /// which only its reshaping makes HDR.
    pub base_hdr: Option<Hdr>,
    /// Cover art in an audio file, not a video.
    pub picture: bool,
}

#[derive(Clone, Debug)]
pub struct AudioInfo {
    pub codec: AudioCodec,
    pub channels: u16,
    pub sample_rate: u32,
}

impl MediaInfo {
    /// Read what `input` holds; `path` tells WebM from Matroska.
    pub(crate) fn read(input: &Input, path: &str) -> Self {
        let streams = input
            .streams()
            .map(|stream| {
                let parameters = stream.parameters();
                let metadata = stream.metadata();
                let disposition = stream.disposition();
                let kind = match parameters.medium() {
                    Type::Video => StreamKind::Video(video_info(&stream)),
                    Type::Audio => StreamKind::Audio(audio_info(&parameters)),
                    Type::Subtitle => StreamKind::Subtitle {
                        text: is_text(parameters.id()),
                    },
                    _ => StreamKind::Other,
                };
                StreamInfo {
                    index: stream.index(),
                    kind,
                    language: tag(&metadata, c"language").filter(|language| language != "und"),
                    title: tag(&metadata, c"title"),
                    default: disposition.contains(ffmpeg::format::stream::Disposition::DEFAULT),
                }
            })
            .collect();
        // SAFETY: the context is open while `input` is.
        let (start, bit_rate) = unsafe {
            let context = &*input.as_ptr();
            (context.start_time, context.bit_rate)
        };
        let duration = input.duration();
        Self {
            container: container(input.format().name(), path),
            duration: (duration > 0).then(|| duration as f64 / f64::from(ffi::AV_TIME_BASE)),
            start: if start == ffi::AV_NOPTS_VALUE {
                0.0
            } else {
                start as f64 / f64::from(ffi::AV_TIME_BASE)
            },
            bit_rate: (bit_rate > 0).then_some(bit_rate as u64),
            streams,
        }
    }

    pub fn stream(&self, index: usize) -> Option<&StreamInfo> {
        self.streams.iter().find(|stream| stream.index == index)
    }

    pub fn video(&self, index: usize) -> Option<&VideoInfo> {
        match &self.stream(index)?.kind {
            StreamKind::Video(video) => Some(video),
            _ => None,
        }
    }

    pub fn audio(&self, index: usize) -> Option<&AudioInfo> {
        match &self.stream(index)?.kind {
            StreamKind::Audio(audio) => Some(audio),
            _ => None,
        }
    }

    /// The stream of a kind a device playing the file itself would pick:
    /// the one marked default, else the first.
    pub(crate) fn first(&self, video: bool) -> Option<usize> {
        let of_kind = |stream: &&StreamInfo| match &stream.kind {
            StreamKind::Video(info) => video && !info.picture,
            StreamKind::Audio(_) => !video,
            _ => false,
        };
        self.streams
            .iter()
            .filter(of_kind)
            .find(|stream| stream.default)
            .or_else(|| self.streams.iter().find(of_kind))
            .map(|stream| stream.index)
    }
}

fn container(name: &str, path: &str) -> Container {
    let lower = path.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or_default();
    match name {
        "mov,mp4,m4a,3gp,3g2,mj2" => Container::Mp4,
        "matroska,webm" if Path::new(path).extension().is_some_and(|ext| ext == "webm") => {
            Container::WebM
        }
        "matroska,webm" => Container::Matroska,
        "mpegts" => Container::MpegTs,
        "avi" => Container::Avi,
        "mp3" => Container::Mp3,
        "aac" => Container::Aac,
        "flac" => Container::Flac,
        "ogg" => Container::Ogg,
        "wav" => Container::Wav,
        "hls" => Container::Hls,
        _ => Container::Other,
    }
}

fn video_info(stream: &ffmpeg::format::stream::Stream) -> VideoInfo {
    let parameters = stream.parameters();
    // SAFETY: the parameters belong to an open stream. They are read through
    // the pointer, never a reference, and the transfer and color space as
    // the C ints they are: a file can hold numbers the Rust enums have no
    // variant for.
    let (width, height, format, transfer, space, dolby_vision, picture) = unsafe {
        let raw = parameters.as_ptr();
        let dolby_vision = !ffi::av_packet_side_data_get(
            (*raw).coded_side_data,
            (*raw).nb_coded_side_data,
            ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
        )
        .is_null();
        let disposition = (*stream.as_ptr()).disposition;
        (
            (*raw).width.max(0) as u32,
            (*raw).height.max(0) as u32,
            (*raw).format,
            (&raw const (*raw).color_trc).cast::<u32>().read(),
            (&raw const (*raw).color_space).cast::<u32>().read(),
            dolby_vision,
            disposition & ffi::AV_DISPOSITION_ATTACHED_PIC != 0,
        )
    };
    let pq = transfer == ffi::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE2084 as u32;
    let hlg = transfer == ffi::AVColorTransferCharacteristic::AVCOL_TRC_ARIB_STD_B67 as u32;
    let hdr = if dolby_vision {
        Some(Hdr::DolbyVision)
    } else if pq {
        Some(Hdr::Hdr10)
    } else if hlg {
        Some(Hdr::Hlg)
    } else {
        None
    };
    let ipt = space == ffi::AVColorSpace::AVCOL_SPC_IPT_C2 as u32;
    let base_hdr = match hdr {
        Some(Hdr::DolbyVision) if pq && !ipt => Some(Hdr::Hdr10),
        Some(Hdr::DolbyVision) if hlg => Some(Hdr::Hlg),
        Some(Hdr::DolbyVision) => None,
        other => other,
    };
    let rate = stream.avg_frame_rate();
    VideoInfo {
        codec: video_codec(parameters.id()),
        width,
        height,
        bit_depth: bit_depth(format),
        frame_rate: (rate.numerator() > 0 && rate.denominator() > 0).then(|| f64::from(rate)),
        hdr,
        base_hdr,
        picture,
    }
}

/// Bits per sample of a pixel format, by its first component; 8 when unknown.
fn bit_depth(format: i32) -> u8 {
    // Only a format the Rust enum has a variant for is made one.
    if !(0..ffi::AVPixelFormat::AV_PIX_FMT_NB as i32).contains(&format) {
        return 8;
    }
    // SAFETY: the enum's variants run without a gap from NONE (-1) to NB,
    // and FFmpeg's descriptor table is for any of them.
    unsafe {
        let descriptor =
            ffi::av_pix_fmt_desc_get(std::mem::transmute::<i32, ffi::AVPixelFormat>(format));
        if descriptor.is_null() {
            8
        } else {
            (*descriptor).comp[0].depth.clamp(1, 16) as u8
        }
    }
}

fn audio_info(parameters: &ffmpeg::codec::Parameters) -> AudioInfo {
    // SAFETY: the parameters belong to an open stream. They are read
    // through the pointer, never a reference, as `video_info` says.
    let (channels, sample_rate) = unsafe {
        let raw = parameters.as_ptr();
        (
            (*raw).ch_layout.nb_channels.max(0) as u16,
            (*raw).sample_rate.max(0) as u32,
        )
    };
    AudioInfo {
        codec: audio_codec(parameters.id()),
        channels,
        sample_rate,
    }
}

pub(crate) fn video_codec(id: Id) -> VideoCodec {
    match id {
        Id::H264 => VideoCodec::H264,
        Id::HEVC => VideoCodec::Hevc,
        Id::VP8 => VideoCodec::Vp8,
        Id::VP9 => VideoCodec::Vp9,
        Id::AV1 => VideoCodec::Av1,
        Id::MPEG2VIDEO => VideoCodec::Mpeg2,
        _ => VideoCodec::Other,
    }
}

pub(crate) fn audio_codec(id: Id) -> AudioCodec {
    match id {
        Id::AAC => AudioCodec::Aac,
        Id::AC3 => AudioCodec::Ac3,
        Id::EAC3 => AudioCodec::Eac3,
        Id::MP3 => AudioCodec::Mp3,
        Id::OPUS => AudioCodec::Opus,
        Id::VORBIS => AudioCodec::Vorbis,
        Id::FLAC => AudioCodec::Flac,
        Id::ALAC => AudioCodec::Alac,
        Id::PCM_S16LE
        | Id::PCM_S16BE
        | Id::PCM_S24LE
        | Id::PCM_S24BE
        | Id::PCM_S32LE
        | Id::PCM_F32LE
        | Id::PCM_U8 => AudioCodec::Pcm,
        _ => AudioCodec::Other,
    }
}

/// Subtitle codecs whose cues are text.
fn is_text(id: Id) -> bool {
    matches!(
        id,
        Id::SUBRIP | Id::SRT | Id::ASS | Id::SSA | Id::MOV_TEXT | Id::WEBVTT | Id::TEXT
    )
}

#[cfg(test)]
#[path = "tests/media.rs"]
mod tests;
