//! Deciding what a device gets: the least work that fits what it plays.
//! The file or stream as it is, if it can; else HLS or an MPEG-TS stream,
//! with each track copied when the device decodes it and re-encoded when
//! it doesn't. Picture subtitles picked from the media are drawn into the
//! video, and adjustments done to its picture, which is re-encoded for
//! them where it can be; where it can't, it's copied without them. A sound
//! delay has the media sent other than as it is, where it can be.

use crate::Error;
use crate::adjust::Adjustments;
use crate::encode;
use crate::media::{MediaInfo, Source, StreamKind, VideoInfo};
use crate::mux::Format;
use crate::profile::{AudioCodec, Container, Hdr, Profile, Segments, VideoCodec};
use crate::server::CastRequest;
use crate::subtitles::SubtitleSource;

/// Which stream of a kind to play.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pick {
    /// The one the file marks as default, else the first.
    #[default]
    Best,
    Stream(usize),
    /// None of this kind.
    Off,
}

/// How the device gets the media.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// The stream's own URL, which the device fetches itself.
    Direct,
    /// The file as it is, with Range requests.
    File,
    /// An HLS playlist of segments made as the device asks for them.
    Hls(Segments),
    /// A progressive MPEG-TS stream, from where the device asks.
    Stream,
    /// The sound alone, a progressive stream in this container (AAC or
    /// FLAC), from where the device asks.
    Sound(Container),
}

#[derive(Clone, PartialEq, Debug)]
pub enum VideoPlan {
    Copy {
        stream: usize,
        codec: VideoCodec,
    },
    Encode {
        stream: usize,
        from: VideoCodec,
        to: VideoCodec,
        /// FFmpeg's name for the hardware encoder.
        encoder: &'static str,
        width: u32,
        height: u32,
        ten_bit: bool,
        /// Kept, where the device shows it and the encoding is 10-bit:
        /// HDR10 or HLG, as Dolby Vision's base layer is too, its reshaping
        /// lost, or HDR10 that IPT is reshaped into. HDR not kept is fitted
        /// to SDR.
        hdr: Option<Hdr>,
        bit_rate: u64,
    },
}

#[derive(Clone, PartialEq, Debug)]
pub enum AudioPlan {
    Copy {
        stream: usize,
        codec: AudioCodec,
    },
    Encode {
        stream: usize,
        from: AudioCodec,
        to: AudioCodec,
        channels: u16,
        sample_rate: u32,
        bit_rate: u64,
    },
}

/// What a cast serves.
#[derive(Clone, PartialEq, Debug)]
pub struct Plan {
    pub delivery: Delivery,
    pub video: Option<VideoPlan>,
    pub audio: Option<AudioPlan>,
    /// The picture subtitle stream drawn into the video, by index.
    pub subtitles: Option<usize>,
    /// Seconds the sound is sent after the picture (negative: before it).
    pub audio_delay: f64,
    /// What's done to the picture as it's re-encoded.
    pub adjustments: Option<Adjustments>,
}

impl Plan {
    /// A few words on what happens to the media, for a person:
    /// "as is", "repackaged", "re-encoded to H.264, subtitles drawn in".
    pub fn describe(&self) -> String {
        if matches!(self.delivery, Delivery::Direct | Delivery::File) {
            return "as is".into();
        }
        let done = self.what_is_done();
        match self.subtitles {
            Some(_) => format!("{done}, subtitles drawn in"),
            None => done,
        }
    }

    fn what_is_done(&self) -> String {
        match (&self.video, &self.audio) {
            (Some(VideoPlan::Encode { to, .. }), Some(AudioPlan::Encode { to: audio, .. })) => {
                format!(
                    "re-encoded to {} and {}",
                    video_name(*to),
                    audio_name(*audio)
                )
            }
            (Some(VideoPlan::Encode { to, .. }), _) => format!("re-encoded to {}", video_name(*to)),
            (_, Some(AudioPlan::Encode { to, .. })) => {
                format!("repackaged, sound re-encoded to {}", audio_name(*to))
            }
            _ => "repackaged".into(),
        }
    }

    pub(crate) fn video_stream(&self) -> Option<usize> {
        self.video.as_ref().map(|video| match video {
            VideoPlan::Copy { stream, .. } | VideoPlan::Encode { stream, .. } => *stream,
        })
    }

    /// The HDR the video sent shows, repackaged or re-encoded: what an
    /// encoding keeps, or a copy's base layer's, as a copy loses its Dolby
    /// Vision signalling. None for SDR.
    pub(crate) fn shown_hdr(&self, media: &MediaInfo) -> Option<Hdr> {
        match self.video.as_ref()? {
            VideoPlan::Copy { stream, .. } => media.video(*stream)?.base_hdr,
            VideoPlan::Encode { hdr, .. } => *hdr,
        }
    }

    pub(crate) fn audio_stream(&self) -> Option<usize> {
        self.audio.as_ref().map(|audio| match audio {
            AudioPlan::Copy { stream, .. } | AudioPlan::Encode { stream, .. } => *stream,
        })
    }

    pub(crate) fn segments(&self) -> Segments {
        match self.delivery {
            Delivery::Hls(segments) => segments,
            _ => Segments::Ts,
        }
    }

    /// What its jobs' muxer writes.
    pub(crate) fn format(&self) -> Format {
        format(self.delivery)
    }
}

/// What the muxer writes for `delivery`.
fn format(delivery: Delivery) -> Format {
    match delivery {
        Delivery::Hls(segments) => segments.into(),
        Delivery::Sound(container) => sound_format(container).unwrap_or(Format::Adts),
        Delivery::Direct | Delivery::File | Delivery::Stream => Format::Ts,
    }
}

/// What the muxer writes for a stream of sound in `container`, where it
/// can.
fn sound_format(container: Container) -> Option<Format> {
    match container {
        Container::Aac => Some(Format::Adts),
        Container::Flac => Some(Format::Flac),
        _ => None,
    }
}

/// The container a speaker gets the sound alone in: one that carries it
/// as it is, else the first it takes that sound can be re-encoded for.
fn sound_container(
    media: &MediaInfo,
    audio: Option<usize>,
    profile: &Profile,
) -> Option<Container> {
    let codec = audio
        .and_then(|index| media.audio(index))
        .map(|audio| audio.codec);
    let mut made = profile
        .sound_stream
        .iter()
        .copied()
        .filter(|container| sound_format(*container).is_some());
    made.clone()
        .find(|container| {
            codec.is_some_and(|codec| {
                profile.audio.contains(&codec)
                    && sound_format(*container).is_some_and(|format| format.carries_audio(codec))
            })
        })
        .or_else(|| made.next())
}

fn video_name(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::H264 => "H.264",
        VideoCodec::Hevc => "HEVC",
        VideoCodec::Vp8 => "VP8",
        VideoCodec::Vp9 => "VP9",
        VideoCodec::Av1 => "AV1",
        VideoCodec::Mpeg2 => "MPEG-2",
        VideoCodec::Other => "video",
    }
}

fn audio_name(codec: AudioCodec) -> &'static str {
    match codec {
        AudioCodec::Aac => "AAC",
        AudioCodec::Ac3 => "AC-3",
        AudioCodec::Eac3 => "E-AC-3",
        AudioCodec::Mp3 => "MP3",
        AudioCodec::Opus => "Opus",
        AudioCodec::Vorbis => "Vorbis",
        AudioCodec::Flac => "FLAC",
        AudioCodec::Alac => "ALAC",
        AudioCodec::Pcm => "PCM",
        AudioCodec::Other => "sound",
    }
}

/// The plan for playing the streams `request` picks of `media` on the
/// device it goes to, with as much of what else it asks as the device can
/// be given: picture subtitles and adjustments have the video re-encoded
/// for them, and a sound delay has the media sent other than as it is.
pub(crate) fn plan(media: &MediaInfo, request: &CastRequest) -> Result<Plan, Error> {
    let draw =
        picture(media, request.subtitles.as_ref()).is_some() || request.adjustments.is_some();
    let delay = request.audio_delay != 0.0;
    if draw
        && let Ok(plan) = planned(media, request, true, delay)
        && matches!(plan.video, Some(VideoPlan::Encode { .. }))
    {
        return Ok(plan);
    }
    if delay && let Ok(plan) = planned(media, request, false, true) {
        return Ok(plan);
    }
    planned(media, request, false, false)
}

/// The plan for `plan`, the video re-encoded if `draw`, and the sound
/// delayed if `delay`.
fn planned(
    media: &MediaInfo,
    request: &CastRequest,
    draw: bool,
    delay: bool,
) -> Result<Plan, Error> {
    let (source, profile) = (&request.source, &request.profile);
    let video = pick(media, request.video, true)?;
    let audio = pick(media, request.audio, false)?;
    if video.is_none() && audio.is_none() {
        return Err(Error::Unsupported("there is nothing to play in it".into()));
    }

    let video_fits =
        video.is_none_or(|index| media.video(index).is_some_and(|v| fits(v, profile, true)));
    let audio_fits = audio.is_none_or(|index| {
        media
            .audio(index)
            .is_some_and(|a| profile.audio.contains(&a.codec) && a.channels <= profile.max_channels)
    });
    // A device playing the file itself plays its default streams.
    let as_picked = video == media.first(true) && audio == media.first(false);
    let delayed = delay && audio.is_some();
    if video_fits && audio_fits && as_picked && !draw && !delayed {
        let delivery = match source {
            Source::Stream { .. }
                if profile.fetches_urls
                    && (profile.files.contains(&media.container)
                        || (media.container == crate::Container::Hls && profile.hls.is_some())) =>
            {
                Some(Delivery::Direct)
            }
            Source::File(_) if profile.files.contains(&media.container) => Some(Delivery::File),
            _ => None,
        };
        if let Some(delivery) = delivery {
            return Ok(Plan {
                delivery,
                subtitles: None,
                video: video.map(|stream| VideoPlan::Copy {
                    stream,
                    codec: media
                        .video(stream)
                        .map(|v| v.codec)
                        .unwrap_or(VideoCodec::Other),
                }),
                audio: audio.map(|stream| AudioPlan::Copy {
                    stream,
                    codec: media
                        .audio(stream)
                        .map(|a| a.codec)
                        .unwrap_or(AudioCodec::Other),
                }),
                audio_delay: 0.0,
                adjustments: None,
            });
        }
    }

    let delivery = match (profile.hls, profile.stream) {
        (Some(segments), _) => Delivery::Hls(segments),
        (None, true) => Delivery::Stream,
        (None, false)
            if let Some(container) =
                sound_container(media, audio, profile).filter(|_| video.is_none()) =>
        {
            Delivery::Sound(container)
        }
        (None, false) => {
            return Err(Error::Unsupported(
                "the device plays neither a file like this, HLS, nor an MPEG-TS stream".into(),
            ));
        }
    };
    let segments = match delivery {
        Delivery::Hls(segments) => segments,
        _ => Segments::Ts,
    };
    let adjustments = request.adjustments.as_ref();
    let video = video
        .map(|stream| match media.video(stream) {
            Some(info) if draw => encoded_video(info, stream, profile, segments, adjustments),
            _ => plan_video(media, stream, profile, segments, adjustments),
        })
        .transpose()?;
    Ok(Plan {
        delivery,
        subtitles: drawn(media, video.as_ref(), request.subtitles.as_ref()),
        audio: audio
            .map(|stream| plan_audio(media, stream, profile, format(delivery)))
            .transpose()?,
        audio_delay: if delayed { request.audio_delay } else { 0.0 },
        adjustments: adjusted(video.as_ref(), request.adjustments),
        video,
    })
}

/// `adjustments`, where `video` is re-encoded for them to be done.
fn adjusted(video: Option<&VideoPlan>, adjustments: Option<Adjustments>) -> Option<Adjustments> {
    adjustments.filter(|_| matches!(video, Some(VideoPlan::Encode { .. })))
}

/// The stream of `subtitles` drawn into `video`: picture subtitles in the
/// media, where the video is re-encoded.
fn drawn(
    media: &MediaInfo,
    video: Option<&VideoPlan>,
    subtitles: Option<&SubtitleSource>,
) -> Option<usize> {
    picture(media, subtitles).filter(|_| matches!(video, Some(VideoPlan::Encode { .. })))
}

/// The picture subtitle stream of `media` that `subtitles` is, if it is
/// one.
fn picture(media: &MediaInfo, subtitles: Option<&SubtitleSource>) -> Option<usize> {
    let Some(SubtitleSource::Track(index)) = subtitles else {
        return None;
    };
    matches!(
        media.stream(*index).map(|stream| &stream.kind),
        Some(StreamKind::Subtitle { text: false })
    )
    .then_some(*index)
}

fn pick(media: &MediaInfo, pick: Pick, video: bool) -> Result<Option<usize>, Error> {
    match pick {
        Pick::Off => Ok(None),
        Pick::Best => Ok(media.first(video)),
        Pick::Stream(index) => match media.stream(index).map(|stream| &stream.kind) {
            Some(StreamKind::Video(info)) if video && !info.picture => Ok(Some(index)),
            Some(StreamKind::Audio(_)) if !video => Ok(Some(index)),
            _ => Err(Error::Unsupported(format!(
                "stream {index} isn't {}",
                if video { "a video" } else { "sound" }
            ))),
        },
    }
}

/// Whether the device plays `video` as it is: in the file it's in
/// (`as_is`), or repackaged, where Dolby Vision loses its signalling and is
/// only its base layer, which IPT's is no picture of.
fn fits(video: &VideoInfo, profile: &Profile, as_is: bool) -> bool {
    let hdr = if as_is {
        video.hdr.is_none_or(|hdr| profile.hdr.contains(&hdr))
    } else {
        !video.ipt && video.base_hdr.is_none_or(|hdr| profile.hdr.contains(&hdr))
    };
    profile.video.contains(&video.codec)
        && video.bit_depth <= profile.max_bit_depth
        && profile
            .max_size
            .is_none_or(|(width, height)| video.width <= width && video.height <= height)
        && hdr
}

fn plan_video(
    media: &MediaInfo,
    stream: usize,
    profile: &Profile,
    segments: Segments,
    adjustments: Option<&Adjustments>,
) -> Result<VideoPlan, Error> {
    let info = media
        .video(stream)
        .ok_or_else(|| Error::Unsupported(format!("stream {stream} isn't a video")))?;
    if fits(info, profile, false) && segments.carries_video(info.codec) {
        return Ok(VideoPlan::Copy {
            stream,
            codec: info.codec,
        });
    }
    encoded_video(info, stream, profile, segments, adjustments)
}

/// Re-encode the video `plan` copies, with the picture subtitles and
/// adjustments `request` asks for it can now have: for video with
/// keyframes too far apart to cut it at.
pub(crate) fn re_encode_video(
    plan: &mut Plan,
    media: &MediaInfo,
    request: &CastRequest,
) -> Result<(), Error> {
    let Some(VideoPlan::Copy { stream, .. }) = plan.video else {
        return Ok(());
    };
    let info = media
        .video(stream)
        .ok_or_else(|| Error::Unsupported(format!("stream {stream} isn't a video")))?;
    let adjustments = request.adjustments.as_ref();
    let segments = plan.segments();
    plan.video = Some(encoded_video(
        info,
        stream,
        &request.profile,
        segments,
        adjustments,
    )?);
    plan.subtitles = drawn(media, plan.video.as_ref(), request.subtitles.as_ref());
    plan.adjustments = adjusted(plan.video.as_ref(), request.adjustments);
    Ok(())
}

/// The video re-encoded at the size it shows at, `adjustments` done.
fn encoded_video(
    info: &VideoInfo,
    stream: usize,
    profile: &Profile,
    segments: Segments,
    adjustments: Option<&Adjustments>,
) -> Result<VideoPlan, Error> {
    let (width, height) = match adjustments {
        Some(adjustments) => {
            let (width, height) = adjustments.shown(info.width, info.height);
            fit_size(
                width.round() as u32,
                height.round() as u32,
                profile.max_size,
            )
        }
        None => fit_size(info.width, info.height, profile.max_size),
    };
    let deep = info.bit_depth > 8 || info.hdr.is_some();
    let mut candidates: Vec<VideoCodec> = profile
        .video
        .iter()
        .copied()
        .filter(|codec| matches!(codec, VideoCodec::H264 | VideoCodec::Hevc))
        .filter(|codec| segments.carries_video(*codec))
        .collect();
    // 10-bit and HDR video keep their depth in HEVC, where the device has it.
    if deep && profile.max_bit_depth >= 10 {
        candidates.sort_by_key(|codec| *codec != VideoCodec::Hevc);
    }
    if candidates.is_empty() {
        return Err(Error::Unsupported(
            "the device decodes no video codec this computer encodes".into(),
        ));
    }
    for to in candidates {
        let ten_bit = deep && to == VideoCodec::Hevc && profile.max_bit_depth >= 10;
        // IPT's reshaping makes HDR10.
        let hdr = info
            .base_hdr
            .or(info.ipt.then_some(Hdr::Hdr10))
            .filter(|hdr| ten_bit && profile.hdr.contains(hdr));
        if let Some(encoder) = encode::video_encoder(to, width, height, ten_bit) {
            return Ok(VideoPlan::Encode {
                stream,
                from: info.codec,
                to,
                encoder,
                width,
                height,
                ten_bit,
                hdr,
                bit_rate: video_bit_rate(width, height, info.frame_rate, to),
            });
        }
    }
    Err(Error::NoEncoder)
}

fn plan_audio(
    media: &MediaInfo,
    stream: usize,
    profile: &Profile,
    format: Format,
) -> Result<AudioPlan, Error> {
    let info = media
        .audio(stream)
        .ok_or_else(|| Error::Unsupported(format!("stream {stream} isn't sound")))?;
    if profile.audio.contains(&info.codec)
        && info.channels <= profile.max_channels
        && format.carries_audio(info.codec)
    {
        return Ok(AudioPlan::Copy {
            stream,
            codec: info.codec,
        });
    }
    // What this computer encodes: FLAC only for a stream of it.
    let encodes = |codec: AudioCodec| match format {
        Format::Flac => codec == AudioCodec::Flac,
        _ => matches!(codec, AudioCodec::Aac | AudioCodec::Ac3 | AudioCodec::Eac3),
    };
    let to = profile
        .audio
        .iter()
        .copied()
        .find(|codec| encodes(*codec) && format.carries_audio(*codec))
        .ok_or_else(|| {
            Error::Unsupported("the device decodes no audio codec this computer encodes".into())
        })?;
    let most = match to {
        AudioCodec::Ac3 => 6,
        _ => 8,
    };
    let channels = info.channels.clamp(1, profile.max_channels.clamp(1, most));
    let sample_rate = match (to, info.sample_rate) {
        (_, rate @ (44_100 | 48_000)) => rate,
        _ => 48_000,
    };
    let per_channel: u64 = if to == AudioCodec::Aac {
        64_000
    } else {
        96_000
    };
    Ok(AudioPlan::Encode {
        stream,
        from: info.codec,
        to,
        channels,
        sample_rate,
        bit_rate: (per_channel * u64::from(channels)).min(640_000),
    })
}

/// The size to encode at: the picture's, or as much of it as `max` holds,
/// in even numbers, as 4:2:0 needs.
pub(crate) fn fit_size(width: u32, height: u32, max: Option<(u32, u32)>) -> (u32, u32) {
    let (width, height) = (width.max(2), height.max(2));
    let scale = max.map_or(1.0, |(max_width, max_height)| {
        (f64::from(max_width) / f64::from(width))
            .min(f64::from(max_height) / f64::from(height))
            .min(1.0)
    });
    let even = |value: u32| ((f64::from(value) * scale / 2.0).round() as u32 * 2).max(2);
    (even(width), even(height))
}

/// Bits a second for re-encoded video: about a tenth of a bit a pixel for
/// H.264, less for HEVC, within sensible bounds.
fn video_bit_rate(width: u32, height: u32, frame_rate: Option<f64>, codec: VideoCodec) -> u64 {
    let pixels = f64::from(width) * f64::from(height) * frame_rate.unwrap_or(30.0).clamp(1.0, 60.0);
    let per_pixel = if codec == VideoCodec::Hevc { 0.07 } else { 0.1 };
    ((pixels * per_pixel) as u64).clamp(1_000_000, 40_000_000)
}

#[cfg(test)]
#[path = "tests/plan.rs"]
mod tests;
