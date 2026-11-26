//! What a file or stream holds, read without playing it, for `plays info`:
//! its format and tags; every stream in it, with its codec and its
//! pictures' size, shape, colours and light, or its sound; its chapters;
//! and the subtitle files beside it that play with it.

use std::collections::BTreeMap;
use std::ffi::c_int;
use std::ptr;

use anyhow::Result;
use ffmpeg::format::stream::{Disposition, Stream};
use ffmpeg::util::color;
use ffmpeg::util::format::Sample;
use ffmpeg::{Rational, codec, ffi, media};
use ffmpeg_next as ffmpeg;
use serde::Serialize;

use super::chapters::Chapter;
use super::colors::Colors;
use super::decoder::{self, container_aspect};
use super::enhancement::Configuration;
use super::orientation::Orientation;
use super::source::Source;
use super::tracks::{self, Track};
use super::{details, external, hdr};

/// FFmpeg's time base for a file's duration.
const AV_TIME_BASE: f64 = 1_000_000.0;

/// What a file or stream holds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Probe {
    /// The file's path, or the stream's address, without a password.
    pub source: String,
    /// Its format, as FFmpeg describes it, such as "QuickTime / MOV".
    pub format: String,
    /// How long it plays, in seconds; none for a live stream, or a file
    /// that doesn't say.
    pub duration: Option<f64>,
    /// How big the file is, in bytes; none for a stream.
    pub size: Option<u64>,
    /// Bits a second, for all of it.
    pub bit_rate: Option<u64>,
    /// Its tags, by name, such as `title` or `encoder`.
    pub tags: BTreeMap<String, String>,
    /// Every stream in it, in its order.
    pub streams: Vec<StreamProbe>,
    /// Its parts, in order, at the file's own times.
    pub chapters: Vec<Chapter>,
    /// The subtitle files beside it that play with it, as Plays finds them.
    pub subtitle_files: Vec<SubtitleFile>,
}

/// What a stream holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Video,
    Audio,
    Subtitle,
    /// A file kept in it, such as a font.
    Attachment,
    /// Anything else, such as timecodes.
    Data,
}

/// A stream in a file.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StreamProbe {
    pub index: usize,
    pub kind: Kind,
    /// FFmpeg's name for its codec, such as `hevc`.
    pub codec: String,
    /// What the codec is, such as "H.265 / HEVC (High Efficiency Video
    /// Coding)".
    pub codec_name: Option<String>,
    /// The container's code for it, such as `hvc1`, where it has one.
    pub codec_tag: Option<String>,
    /// The codec's profile, such as "Main 10".
    pub profile: Option<String>,
    /// Bits a second, where the file says.
    pub bit_rate: Option<u64>,
    /// As its tags say, usually an ISO 639-2 code such as `eng`.
    pub language: Option<String>,
    /// Its title, or an attachment's file name.
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
    pub hearing_impaired: bool,
    /// A picture of what's in the file, its cover, rather than video.
    pub cover: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoProbe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioProbe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<SubtitleProbe>,
}

/// A video stream's pictures.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VideoProbe {
    /// The size they're coded at.
    pub width: u32,
    pub height: u32,
    /// The size they show at, their pixels' shape corrected, before
    /// they're turned.
    pub display_width: u32,
    pub display_height: u32,
    /// The shape of their pixels where they aren't square, such as "4:3":
    /// as the file's container says, or else the pictures.
    pub pixel_aspect: Option<String>,
    /// Frames a second, on average, where the file says.
    pub frame_rate: Option<f64>,
    /// Such as "yuv420p10le".
    pub pixel_format: Option<String>,
    /// Bits each of a pixel's samples has.
    pub bit_depth: Option<u32>,
    /// The Y'CbCr matrix, primaries and transfer, as FFmpeg names them.
    pub matrix: Option<&'static str>,
    pub primaries: Option<&'static str>,
    pub transfer: Option<&'static str>,
    /// "full" or "limited", where it says.
    pub range: Option<&'static str>,
    /// "HDR10" for PQ, or "HLG", where a player without Dolby Vision
    /// shows it so.
    pub hdr: Option<&'static str>,
    pub dolby_vision: Option<DolbyVision>,
    /// The brightest the display it was mastered on gets, in nits.
    pub mastering_peak: Option<f64>,
    /// Its brightest pixel and brightest frame on average, in nits.
    pub max_cll: Option<u32>,
    pub max_fall: Option<u32>,
    /// How far it's turned clockwise as it shows, in degrees, as its
    /// display matrix says, after being mirrored.
    pub rotation: u16,
    pub mirrored: bool,
    /// For interlaced video, which field shows first: "top" or "bottom".
    pub first_field: Option<&'static str>,
}

/// What a stream's Dolby Vision configuration says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DolbyVision {
    /// Its profile as it's named, its base layer's compatibility after a
    /// dot where it has a choice of them: "8.1", "7", "10.4".
    pub name: String,
    pub profile: u8,
    pub level: u8,
    pub compatibility: u8,
    /// The picture a player without Dolby Vision shows.
    pub base_layer: bool,
    /// The second layer profile 7 adds detail with.
    pub enhancement_layer: bool,
}

/// An audio stream's sound.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AudioProbe {
    pub sample_rate: u32,
    pub channels: u32,
    /// Such as "stereo" or "5.1(side)".
    pub layout: Option<String>,
    /// Such as "fltp".
    pub sample_format: Option<&'static str>,
    /// Bits a sample, for codecs that keep them: lossless ones, and PCM.
    pub bits: Option<u32>,
}

/// A subtitle stream's cues.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SubtitleProbe {
    /// Text, rather than pictures.
    pub text: bool,
}

/// A subtitle file beside the file, that plays with it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SubtitleFile {
    pub path: String,
    /// FFmpeg's name for its codec, such as `subrip`.
    pub codec: String,
    /// As its name or itself says.
    pub language: Option<String>,
    pub title: Option<String>,
    pub forced: bool,
    pub hearing_impaired: bool,
}

impl Probe {
    /// What `source` holds, opened as Plays opens it.
    pub fn of(source: &Source) -> Result<Self> {
        let input = decoder::open_input(source, None, || false)?;
        let (size, subtitle_files) = match source {
            Source::File(path) => (
                std::fs::metadata(path).ok().map(|file| file.len()),
                external::tracks(path)
                    .iter()
                    .map(SubtitleFile::of)
                    .collect(),
            ),
            Source::Url(_) => (None, Vec::new()),
        };
        Ok(Self {
            source: source.to_string(),
            format: input.format().description().to_owned(),
            duration: (input.duration() > 0).then(|| input.duration() as f64 / AV_TIME_BASE),
            size,
            bit_rate: details::positive(input.bit_rate()),
            tags: ffmpeg_safe::tags(&input.metadata()).into_iter().collect(),
            streams: input
                .streams()
                .map(|stream| StreamProbe::of(&stream))
                .collect(),
            chapters: decoder::chapters(&input, 0.0),
            subtitle_files,
        })
    }
}

impl StreamProbe {
    fn of(stream: &Stream) -> Self {
        let parameters = stream.parameters();
        let disposition = stream.disposition();
        let metadata = stream.metadata();
        let tag = |key| ffmpeg_safe::tag(&metadata, key);
        // SAFETY: the parameters belong to the stream, which outlives this
        // call.
        let (codec, tag_code, profile, bit_rate) = unsafe {
            let raw = parameters.as_ptr();
            (
                (*raw).codec_id,
                (*raw).codec_tag,
                (*raw).profile,
                (*raw).bit_rate,
            )
        };
        let kind = match parameters.medium() {
            media::Type::Video => Kind::Video,
            media::Type::Audio => Kind::Audio,
            media::Type::Subtitle => Kind::Subtitle,
            media::Type::Attachment => Kind::Attachment,
            _ => Kind::Data,
        };
        let cover = disposition.contains(Disposition::ATTACHED_PIC);
        let (codec_name, text) = details::described(codec);
        Self {
            index: stream.index(),
            kind,
            codec: parameters.id().name().to_owned(),
            codec_name,
            codec_tag: fourcc(tag_code),
            profile: details::profile_name(codec, profile),
            bit_rate: details::positive(bit_rate),
            language: tracks::stream_language(&metadata),
            title: tag(c"title").or_else(|| tag(c"filename")),
            default: disposition.contains(Disposition::DEFAULT),
            forced: disposition.contains(Disposition::FORCED),
            hearing_impaired: disposition.contains(Disposition::HEARING_IMPAIRED),
            cover,
            video: (kind == Kind::Video).then(|| VideoProbe::of(stream, &parameters)),
            audio: (kind == Kind::Audio).then(|| AudioProbe::of(&parameters)),
            subtitle: (kind == Kind::Subtitle).then_some(SubtitleProbe { text }),
        }
    }
}

impl VideoProbe {
    fn of(stream: &Stream, parameters: &codec::Parameters) -> Self {
        // SAFETY: as for the stream's codec. Its field order is read as the
        // number it is.
        let (width, height, format, aspect, field_order) = unsafe {
            let raw = parameters.as_ptr();
            (
                (*raw).width,
                (*raw).height,
                (*raw).format,
                Rational::from((*raw).sample_aspect_ratio),
                (&raw const (*raw).field_order).cast::<u32>().read(),
            )
        };
        let (width, height) = (
            u32::try_from(width).unwrap_or(0),
            u32::try_from(height).unwrap_or(0),
        );
        let aspect = container_aspect(stream)
            .or_else(|| ratio(aspect).map(|_| aspect))
            .filter(|aspect| aspect.numerator() != aspect.denominator());
        let display_width = aspect.map_or(width, |aspect| {
            (f64::from(width) * f64::from(aspect)).round() as u32
        });
        let (pixel_format, bit_depth) = pixel_format(format);
        let colors = Colors::of_parameters(parameters);
        let dolby_vision = Configuration::of(parameters).map(DolbyVision::of);
        let orientation = Orientation::of(parameters);
        let (mastering_peak, max_cll, max_fall) = light(parameters);
        Self {
            width,
            height,
            display_width,
            display_height: height,
            pixel_aspect: aspect
                .map(|aspect| format!("{}:{}", aspect.numerator(), aspect.denominator())),
            frame_rate: ratio(stream.avg_frame_rate()),
            pixel_format,
            bit_depth,
            // None where it's unspecified.
            matrix: colors.space.name(),
            primaries: colors.primaries.name(),
            transfer: colors.transfer.name(),
            range: match colors.range {
                color::Range::JPEG => Some("full"),
                color::Range::MPEG => Some("limited"),
                color::Range::Unspecified => None,
            },
            hdr: hdr_name(colors.transfer, dolby_vision.as_ref()),
            dolby_vision,
            mastering_peak,
            max_cll,
            max_fall,
            rotation: u16::from(orientation.clockwise_turns()) * 90,
            mirrored: orientation.is_mirrored(),
            first_field: first_field(field_order),
        }
    }
}

/// A container's code for a codec as the four characters it is, if they
/// are characters.
fn fourcc(code: u32) -> Option<String> {
    let characters = code.to_le_bytes();
    characters
        .iter()
        .all(|c| c.is_ascii_graphic() || *c == b' ')
        .then(|| String::from_utf8_lossy(&characters).trim_end().to_owned())
}

/// `rational` as a number, if it is one above nothing.
fn ratio(rational: Rational) -> Option<f64> {
    (rational.numerator() > 0 && rational.denominator() > 0).then(|| f64::from(rational))
}

/// The pixel format numbered `format`, by its name, and the bits its first
/// component has, if FFmpeg knows of one by that number.
fn pixel_format(format: c_int) -> (Option<String>, Option<u32>) {
    let mut descriptor = ptr::null();
    // SAFETY: FFmpeg's table of pixel formats lives as long as the
    // program, and each is looked at by its id, a number, so a file's
    // number that names none names none of them.
    unsafe {
        loop {
            descriptor = ffi::av_pix_fmt_desc_next(descriptor);
            if descriptor.is_null() {
                return (None, None);
            }
            if ffi::av_pix_fmt_desc_get_id(descriptor) as c_int == format {
                let depth = (*descriptor).comp[0].depth;
                return (
                    details::string((*descriptor).name),
                    u32::try_from(depth).ok().filter(|depth| *depth > 0),
                );
            }
        }
    }
}

/// Which field of an interlaced picture shows first, by FFmpeg's field
/// order: none for progressive video, or video that doesn't say.
fn first_field(order: u32) -> Option<&'static str> {
    use ffi::AVFieldOrder::*;
    match order {
        _ if order == AV_FIELD_TT as u32 || order == AV_FIELD_BT as u32 => Some("top"),
        _ if order == AV_FIELD_BB as u32 || order == AV_FIELD_TB as u32 => Some("bottom"),
        _ => None,
    }
}

/// What a player without Dolby Vision shows the video as: HDR10 for PQ
/// and HLG, unless its Dolby Vision has a base layer nothing else shows.
fn hdr_name(
    transfer: color::TransferCharacteristic,
    dolby_vision: Option<&DolbyVision>,
) -> Option<&'static str> {
    if dolby_vision.is_some_and(|dolby_vision| dolby_vision.compatibility == 0) {
        return None;
    }
    match transfer {
        color::TransferCharacteristic::SMPTE2084 => Some("HDR10"),
        color::TransferCharacteristic::ARIB_STD_B67 => Some("HLG"),
        _ => None,
    }
}

impl DolbyVision {
    fn of(configuration: Configuration) -> Self {
        // Profiles 8 to 10 have a base layer of a choice of kinds.
        let name = match configuration.profile {
            8..=10 => format!("{}.{}", configuration.profile, configuration.compatibility),
            profile => profile.to_string(),
        };
        Self {
            name,
            profile: configuration.profile,
            level: configuration.level,
            compatibility: configuration.compatibility,
            base_layer: configuration.base,
            enhancement_layer: configuration.enhancement,
        }
    }
}

/// The peak of the display the video was mastered on, and its MaxCLL and
/// MaxFALL, as its stream's HDR10 metadata says.
fn light(parameters: &codec::Parameters) -> (Option<f64>, Option<u32>, Option<u32>) {
    use ffi::AVPacketSideDataType::*;
    // SAFETY: FFmpeg keeps these kinds of side data as these structs.
    let (display, light) = unsafe {
        (
            side_data::<hdr::sys::MasteringDisplay>(
                parameters,
                AV_PKT_DATA_MASTERING_DISPLAY_METADATA,
            ),
            side_data::<hdr::sys::ContentLightLevel>(parameters, AV_PKT_DATA_CONTENT_LIGHT_LEVEL),
        )
    };
    let peak = display
        .filter(|display| display.has_luminance != 0)
        .and_then(|display| ratio(display.max_luminance.into()));
    let level = |nits: u32| (nits > 0).then_some(nits);
    (
        peak,
        light.as_ref().and_then(|light| level(light.max_cll)),
        light.as_ref().and_then(|light| level(light.max_fall)),
    )
}

/// A copy of `parameters`' side data of `kind`, if it has some as big as a
/// `T`.
///
/// # Safety
/// Side data of `kind` is a `T`.
unsafe fn side_data<T>(
    parameters: &codec::Parameters,
    kind: ffi::AVPacketSideDataType,
) -> Option<T> {
    // SAFETY: the parameters' side data stays valid while they do; its
    // size is checked before it's read, unaligned.
    unsafe {
        let raw = parameters.as_ptr();
        let side =
            ffi::av_packet_side_data_get((*raw).coded_side_data, (*raw).nb_coded_side_data, kind);
        if side.is_null() || (*side).data.is_null() || (*side).size < size_of::<T>() {
            return None;
        }
        Some(ptr::read_unaligned((*side).data.cast::<T>()))
    }
}

impl AudioProbe {
    fn of(parameters: &codec::Parameters) -> Self {
        // SAFETY: as for the stream's codec; its sample format is read as
        // the number it is.
        unsafe {
            let raw = parameters.as_ptr();
            Self {
                sample_rate: u32::try_from((*raw).sample_rate).unwrap_or(0),
                channels: u32::try_from((*raw).ch_layout.nb_channels).unwrap_or(0),
                layout: details::layout_name(&(*raw).ch_layout),
                sample_format: sample_format((*raw).format),
                bits: details::sample_bits((*raw).bits_per_raw_sample, (*raw).codec_id),
            }
        }
    }
}

/// The sample format numbered `format`, by its name.
fn sample_format(format: c_int) -> Option<&'static str> {
    use ffi::AVSampleFormat::*;
    let named = [
        AV_SAMPLE_FMT_U8,
        AV_SAMPLE_FMT_S16,
        AV_SAMPLE_FMT_S32,
        AV_SAMPLE_FMT_FLT,
        AV_SAMPLE_FMT_DBL,
        AV_SAMPLE_FMT_U8P,
        AV_SAMPLE_FMT_S16P,
        AV_SAMPLE_FMT_S32P,
        AV_SAMPLE_FMT_FLTP,
        AV_SAMPLE_FMT_DBLP,
        AV_SAMPLE_FMT_S64,
        AV_SAMPLE_FMT_S64P,
    ];
    let found = named.into_iter().find(|&named| named as c_int == format)?;
    Some(Sample::from(found).name())
}

impl SubtitleFile {
    fn of(track: &Track) -> Self {
        Self {
            path: track
                .file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            codec: track.codec.clone(),
            language: track.language.clone(),
            title: track.title.clone(),
            forced: track.forced,
            hearing_impaired: track.hearing_impaired,
        }
    }
}

#[cfg(test)]
#[path = "tests/probe.rs"]
pub(crate) mod tests;
