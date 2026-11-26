//! What a device plays, as its protocol's crate knows it: the containers it
//! takes as files, whether it takes HLS or an MPEG-TS stream, and the codecs,
//! sizes and HDR kinds it decodes.

/// A container, as files and streams come in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Container {
    Mp4,
    Matroska,
    WebM,
    MpegTs,
    Avi,
    Mp3,
    Aac,
    Flac,
    Ogg,
    Wav,
    Hls,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum VideoCodec {
    H264,
    Hevc,
    Vp8,
    Vp9,
    Av1,
    Mpeg2,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AudioCodec {
    Aac,
    Ac3,
    Eac3,
    Mp3,
    Opus,
    Vorbis,
    Flac,
    Alac,
    Pcm,
    Other,
}

/// High dynamic range, by how it is carried.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Hdr {
    Hdr10,
    Hlg,
    DolbyVision,
}

/// The segments an HLS playlist points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Segments {
    /// Fragmented MP4 (CMAF), which HEVC and HDR need with Apple's players.
    Fmp4,
    /// MPEG-TS, which older players take.
    Ts,
}

/// What a device plays.
#[derive(Clone, PartialEq, Debug)]
pub struct Profile {
    /// Containers it plays as files, fetched as they are with Range requests.
    pub files: Vec<Container>,
    /// HLS, with segments of this kind.
    pub hls: Option<Segments>,
    /// Takes subtitles from the HLS playlist. One given them beside the
    /// media, as a Chromecast is, finds none there, so as not to show two.
    pub hls_subtitles: bool,
    /// A progressive MPEG-TS stream, as DLNA TVs take.
    pub stream: bool,
    /// Containers it takes a progressive stream of the sound alone in, as
    /// internet radio sends it, the one to re-encode to first: AAC (with
    /// ADTS headers) or FLAC. A video's sound reaches a speaker that
    /// otherwise takes only files this way.
    pub sound_stream: Vec<Container>,
    /// Fetches http(s) URLs itself, so a stream that fits is handed over as is.
    pub fetches_urls: bool,
    /// Video codecs it decodes, the one to re-encode to first.
    pub video: Vec<VideoCodec>,
    /// Audio codecs it decodes, the one to re-encode to first.
    pub audio: Vec<AudioCodec>,
    /// The largest picture it shows.
    pub max_size: Option<(u32, u32)>,
    /// Bits per color sample it decodes, 8 or 10.
    pub max_bit_depth: u8,
    pub hdr: Vec<Hdr>,
    /// Of Dolby Vision, where `hdr` has it, the profiles it shows in a file
    /// as it is, as a TV lists those it decodes; `None`, any.
    pub dolby_vision: Option<Vec<u8>>,
    pub max_channels: u16,
}

impl Default for Profile {
    /// What nearly every TV and streaming stick plays: H.264 up to 1080p and
    /// AAC stereo, in MP4 files or HLS of MPEG-TS segments.
    fn default() -> Self {
        Self {
            files: vec![Container::Mp4],
            hls: Some(Segments::Ts),
            hls_subtitles: true,
            stream: false,
            sound_stream: Vec::new(),
            fetches_urls: true,
            video: vec![VideoCodec::H264],
            audio: vec![AudioCodec::Aac],
            max_size: Some((1920, 1080)),
            max_bit_depth: 8,
            hdr: Vec::new(),
            dolby_vision: None,
            max_channels: 2,
        }
    }
}

impl Container {
    /// The MIME type a device is told for a file of this container.
    pub fn mime(self, has_video: bool) -> &'static str {
        match (self, has_video) {
            (Self::Mp4, true) => "video/mp4",
            (Self::Mp4, false) => "audio/mp4",
            (Self::Matroska, true) => "video/x-matroska",
            (Self::Matroska, false) => "audio/x-matroska",
            (Self::WebM, true) => "video/webm",
            (Self::WebM, false) => "audio/webm",
            (Self::MpegTs, _) => "video/mp2t",
            (Self::Avi, _) => "video/x-msvideo",
            (Self::Mp3, _) => "audio/mpeg",
            (Self::Aac, _) => "audio/aac",
            (Self::Flac, _) => "audio/flac",
            (Self::Ogg, _) => "audio/ogg",
            (Self::Wav, _) => "audio/wav",
            (Self::Hls, _) => HLS_MIME,
            (Self::Other, _) => "application/octet-stream",
        }
    }
}

pub(crate) const HLS_MIME: &str = "application/vnd.apple.mpegurl";

impl Segments {
    /// Whether segments of this kind can carry the video codec as it is.
    pub(crate) fn carries_video(self, codec: VideoCodec) -> bool {
        match self {
            Self::Fmp4 => matches!(
                codec,
                VideoCodec::H264 | VideoCodec::Hevc | VideoCodec::Av1 | VideoCodec::Vp9
            ),
            Self::Ts => matches!(
                codec,
                VideoCodec::H264 | VideoCodec::Hevc | VideoCodec::Mpeg2
            ),
        }
    }

    /// Whether segments of this kind can carry the audio codec as it is.
    pub(crate) fn carries_audio(self, codec: AudioCodec) -> bool {
        match self {
            Self::Fmp4 => matches!(
                codec,
                AudioCodec::Aac
                    | AudioCodec::Ac3
                    | AudioCodec::Eac3
                    | AudioCodec::Mp3
                    | AudioCodec::Opus
                    | AudioCodec::Flac
                    | AudioCodec::Alac
            ),
            Self::Ts => matches!(
                codec,
                AudioCodec::Aac | AudioCodec::Ac3 | AudioCodec::Eac3 | AudioCodec::Mp3
            ),
        }
    }
}
