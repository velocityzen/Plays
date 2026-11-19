//! Deciding what a device gets: the least work that fits what it plays.
//! The file or stream as it is, if it can; else HLS or an MPEG-TS stream,
//! with each track copied when the device decodes it and re-encoded when
//! it doesn't. Picture subtitles picked from the media are drawn into video
//! being re-encoded; video copied goes without them.

use crate::Error;
use crate::encode;
use crate::media::{MediaInfo, Source, StreamKind, VideoInfo};
use crate::profile::{AudioCodec, Hdr, Profile, Segments, VideoCodec};
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
        /// Kept, where the device shows it and the encoding is 10-bit.
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

/// The plan for playing the picked streams of `media`, with `subtitles`,
/// on a device that plays what `profile` says.
pub(crate) fn plan(
    media: &MediaInfo,
    source: &Source,
    video: Pick,
    audio: Pick,
    subtitles: Option<&SubtitleSource>,
    profile: &Profile,
) -> Result<Plan, Error> {
    let video = pick(media, video, true)?;
    let audio = pick(media, audio, false)?;
    if video.is_none() && audio.is_none() {
        return Err(Error::Unsupported("there is nothing to play in it".into()));
    }

    let video_fits = video.is_none_or(|index| media.video(index).is_some_and(|v| fits(v, profile)));
    let audio_fits = audio.is_none_or(|index| {
        media
            .audio(index)
            .is_some_and(|a| profile.audio.contains(&a.codec) && a.channels <= profile.max_channels)
    });
    // A device playing the file itself plays its default streams.
    let as_picked = video == media.first(true) && audio == media.first(false);
    if video_fits && audio_fits && as_picked {
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
            });
        }
    }

    let delivery = match (profile.hls, profile.stream) {
        (Some(segments), _) => Delivery::Hls(segments),
        (None, true) => Delivery::Stream,
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
    let video = video
        .map(|stream| plan_video(media, stream, profile, segments))
        .transpose()?;
    Ok(Plan {
        delivery,
        subtitles: drawn(media, video.as_ref(), subtitles),
        audio: audio
            .map(|stream| plan_audio(media, stream, profile, segments))
            .transpose()?,
        video,
    })
}

/// The stream of `subtitles` drawn into `video`: picture subtitles in the
/// media, where the video is re-encoded.
pub(crate) fn drawn(
    media: &MediaInfo,
    video: Option<&VideoPlan>,
    subtitles: Option<&SubtitleSource>,
) -> Option<usize> {
    let Some(SubtitleSource::Track(index)) = subtitles else {
        return None;
    };
    let picture = matches!(
        media.stream(*index).map(|stream| &stream.kind),
        Some(StreamKind::Subtitle { text: false })
    );
    (picture && matches!(video, Some(VideoPlan::Encode { .. }))).then_some(*index)
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

fn fits(video: &VideoInfo, profile: &Profile) -> bool {
    profile.video.contains(&video.codec)
        && video.bit_depth <= profile.max_bit_depth
        && profile
            .max_size
            .is_none_or(|(width, height)| video.width <= width && video.height <= height)
        && video.hdr.is_none_or(|hdr| profile.hdr.contains(&hdr))
}

fn plan_video(
    media: &MediaInfo,
    stream: usize,
    profile: &Profile,
    segments: Segments,
) -> Result<VideoPlan, Error> {
    let info = media
        .video(stream)
        .ok_or_else(|| Error::Unsupported(format!("stream {stream} isn't a video")))?;
    if fits(info, profile) && segments.carries_video(info.codec) {
        return Ok(VideoPlan::Copy {
            stream,
            codec: info.codec,
        });
    }

    let (width, height) = fit_size(info.width, info.height, profile.max_size);
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
        let hdr = info
            .hdr
            .filter(|hdr| ten_bit && *hdr != Hdr::DolbyVision && profile.hdr.contains(hdr));
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
    segments: Segments,
) -> Result<AudioPlan, Error> {
    let info = media
        .audio(stream)
        .ok_or_else(|| Error::Unsupported(format!("stream {stream} isn't sound")))?;
    if profile.audio.contains(&info.codec)
        && info.channels <= profile.max_channels
        && segments.carries_audio(info.codec)
    {
        return Ok(AudioPlan::Copy {
            stream,
            codec: info.codec,
        });
    }
    let to = profile
        .audio
        .iter()
        .copied()
        .find(|codec| {
            matches!(codec, AudioCodec::Aac | AudioCodec::Ac3 | AudioCodec::Eac3)
                && segments.carries_audio(*codec)
        })
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
mod tests {
    use super::*;

    #[test]
    fn fits_the_picture_in_even_numbers() {
        assert_eq!(fit_size(1920, 1080, None), (1920, 1080));
        assert_eq!(fit_size(3840, 2160, Some((1920, 1080))), (1920, 1080));
        assert_eq!(fit_size(1920, 800, Some((1280, 720))), (1280, 534));
        assert_eq!(fit_size(161, 121, None), (162, 122));
    }
}
