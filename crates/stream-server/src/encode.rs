//! Re-encoding, for tracks a device doesn't decode as they are. Video goes
//! through the computer's hardware encoder only: VideoToolbox on macOS,
//! NVENC or VA-API on Linux. Sound goes through FFmpeg's own AAC, AC-3 and
//! E-AC-3 encoders.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use ffmpeg::codec::{self, Flags};
use ffmpeg::ffi;
use ffmpeg::format::{Pixel, Sample};
use ffmpeg::software::{resampling, scaling};
use ffmpeg::util::channel_layout::ChannelLayout;
use ffmpeg::{Dictionary, Packet, Rational, frame, picture};
use ffmpeg_next as ffmpeg;

use crate::Error;
use crate::profile::{AudioCodec, VideoCodec};

/// The hardware encoders to try for a codec, in order.
fn video_encoders(codec: VideoCodec) -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    match codec {
        VideoCodec::H264 => &["h264_videotoolbox"],
        VideoCodec::Hevc => &["hevc_videotoolbox"],
        _ => &[],
    }
    #[cfg(not(target_os = "macos"))]
    match codec {
        VideoCodec::H264 => &["h264_nvenc", "h264_vaapi"],
        VideoCodec::Hevc => &["hevc_nvenc", "hevc_vaapi"],
        _ => &[],
    }
}

/// The hardware encoder that opens here for `codec`, if any: one that has
/// opened is had from then on; one that didn't isn't tried again at that
/// size for a while.
pub(crate) fn video_encoder(
    codec: VideoCodec,
    width: u32,
    height: u32,
    ten_bit: bool,
) -> Option<&'static str> {
    static PROBED: OnceLock<Mutex<Probed>> = OnceLock::new();
    let probed = PROBED.get_or_init(Default::default);
    let mut probed = probed
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    probed.encoder(codec, (width, height), ten_bit, Instant::now(), |name| {
        let settings = VideoSettings {
            encoder: name,
            codec,
            width,
            height,
            ten_bit,
            bit_rate: 2_000_000,
            time_base: Rational::new(1, 25),
            frame_rate: Some(Rational::new(25, 1)),
            colors: Colors::default(),
            global_header: true,
        };
        match VideoEncoder::open(&settings) {
            Ok(_) => true,
            Err(err) => {
                log::info!("{name} doesn't open here: {err}");
                false
            }
        }
    })
}

/// How long an encoder that didn't open at a size isn't tried there again:
/// the hardware's sessions can be in use for a while.
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// What opening the hardware encoders has shown.
#[derive(Default)]
struct Probed {
    /// Each codec and depth's encoder, once one has opened.
    found: HashMap<(VideoCodec, bool), &'static str>,
    /// When none opened, by codec, depth and size.
    failed: HashMap<(VideoCodec, bool, (u32, u32)), Instant>,
}

impl Probed {
    /// The first of `codec`'s encoders that `opens` at `size`, or none.
    fn encoder(
        &mut self,
        codec: VideoCodec,
        size: (u32, u32),
        ten_bit: bool,
        now: Instant,
        mut opens: impl FnMut(&'static str) -> bool,
    ) -> Option<&'static str> {
        if let Some(&name) = self.found.get(&(codec, ten_bit)) {
            return Some(name);
        }
        self.failed
            .retain(|_, at| now.saturating_duration_since(*at) < RETRY_AFTER);
        if self.failed.contains_key(&(codec, ten_bit, size)) {
            return None;
        }
        let found = video_encoders(codec)
            .iter()
            .copied()
            .find(|name| opens(name));
        match found {
            Some(name) => {
                self.found.insert((codec, ten_bit), name);
            }
            None => {
                self.failed.insert((codec, ten_bit, size), now);
            }
        }
        found
    }
}

/// How a picture's colors are coded, kept from the source.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Colors {
    pub primaries: Option<ffi::AVColorPrimaries>,
    pub transfer: Option<ffi::AVColorTransferCharacteristic>,
    pub space: Option<ffi::AVColorSpace>,
}

impl Colors {
    /// A stream's colors, as its parameters give them: ones unspecified,
    /// reserved or without a name in FFmpeg are left unknown.
    pub fn of(parameters: &codec::Parameters) -> Self {
        // SAFETY: the parameters are valid while borrowed. The fields are
        // read as the C ints they are, not as the Rust enums bound to them,
        // since a file can hold numbers the enums have no variant for.
        let (primaries, transfer, space) = unsafe {
            let raw = parameters.as_ptr();
            (
                (&raw const (*raw).color_primaries).cast::<u32>().read(),
                (&raw const (*raw).color_trc).cast::<u32>().read(),
                (&raw const (*raw).color_space).cast::<u32>().read(),
            )
        };
        Self {
            primaries: PRIMARIES
                .into_iter()
                .find(|&named| named as u32 == primaries),
            transfer: TRANSFERS
                .into_iter()
                .find(|&named| named as u32 == transfer),
            space: SPACES.into_iter().find(|&named| named as u32 == space),
        }
    }
}

const PRIMARIES: [ffi::AVColorPrimaries; 11] = {
    use ffi::AVColorPrimaries::*;
    [
        AVCOL_PRI_BT709,
        AVCOL_PRI_BT470M,
        AVCOL_PRI_BT470BG,
        AVCOL_PRI_SMPTE170M,
        AVCOL_PRI_SMPTE240M,
        AVCOL_PRI_FILM,
        AVCOL_PRI_BT2020,
        AVCOL_PRI_SMPTE428,
        AVCOL_PRI_SMPTE431,
        AVCOL_PRI_SMPTE432,
        AVCOL_PRI_EBU3213,
    ]
};

const TRANSFERS: [ffi::AVColorTransferCharacteristic; 16] = {
    use ffi::AVColorTransferCharacteristic::*;
    [
        AVCOL_TRC_BT709,
        AVCOL_TRC_GAMMA22,
        AVCOL_TRC_GAMMA28,
        AVCOL_TRC_SMPTE170M,
        AVCOL_TRC_SMPTE240M,
        AVCOL_TRC_LINEAR,
        AVCOL_TRC_LOG,
        AVCOL_TRC_LOG_SQRT,
        AVCOL_TRC_IEC61966_2_4,
        AVCOL_TRC_BT1361_ECG,
        AVCOL_TRC_IEC61966_2_1,
        AVCOL_TRC_BT2020_10,
        AVCOL_TRC_BT2020_12,
        AVCOL_TRC_SMPTE2084,
        AVCOL_TRC_SMPTE428,
        AVCOL_TRC_ARIB_STD_B67,
    ]
};

const SPACES: [ffi::AVColorSpace; 16] = {
    use ffi::AVColorSpace::*;
    [
        AVCOL_SPC_RGB,
        AVCOL_SPC_BT709,
        AVCOL_SPC_FCC,
        AVCOL_SPC_BT470BG,
        AVCOL_SPC_SMPTE170M,
        AVCOL_SPC_SMPTE240M,
        AVCOL_SPC_YCGCO,
        AVCOL_SPC_BT2020_NCL,
        AVCOL_SPC_BT2020_CL,
        AVCOL_SPC_SMPTE2085,
        AVCOL_SPC_CHROMA_DERIVED_NCL,
        AVCOL_SPC_CHROMA_DERIVED_CL,
        AVCOL_SPC_ICTCP,
        AVCOL_SPC_IPT_C2,
        AVCOL_SPC_YCGCO_RE,
        AVCOL_SPC_YCGCO_RO,
    ]
};

pub(crate) struct VideoSettings<'a> {
    pub encoder: &'a str,
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub ten_bit: bool,
    pub bit_rate: u64,
    /// The time base frames' timestamps come in, which packets keep.
    pub time_base: Rational,
    pub frame_rate: Option<Rational>,
    /// Kept for 10-bit encodings; 8-bit ones are tagged BT.709.
    pub colors: Colors,
    /// Parameter sets in the stream's header, as MP4 needs.
    pub global_header: bool,
}

/// A hardware video encoder, scaling frames to its size and format first.
pub(crate) struct VideoEncoder {
    encoder: ffmpeg::encoder::Video,
    format: Pixel,
    width: u32,
    height: u32,
    scaler: Option<(scaling::Context, (Pixel, u32, u32))>,
    #[cfg(target_os = "linux")]
    vaapi: Option<vaapi::Frames>,
}

// SAFETY: the encoder and scaler are used by one thread at a time, the job's.
unsafe impl Send for VideoEncoder {}

impl VideoEncoder {
    pub fn open(settings: &VideoSettings) -> Result<Self, Error> {
        let codec = ffmpeg::encoder::find_by_name(settings.encoder).ok_or(Error::NoEncoder)?;
        let mut context = codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()
            .map_err(|err| Error::Encode(format!("{}: {err}", settings.encoder)))?;
        // What frames are scaled to before they are handed over.
        let format = if settings.ten_bit {
            Pixel::P010LE
        } else {
            Pixel::NV12
        };
        context.set_width(settings.width);
        context.set_height(settings.height);
        context.set_format(format);
        context.set_time_base(settings.time_base);
        context.set_bit_rate(settings.bit_rate as usize);
        context.set_max_bit_rate(settings.bit_rate as usize * 2);
        context.set_max_b_frames(0);
        // Keyframes come where segments start, when asked for.
        context.set_gop(600);
        if settings.ten_bit {
            if let Some(primaries) = settings.colors.primaries {
                context.set_color_primaries(primaries.into());
            }
            if let Some(transfer) = settings.colors.transfer {
                context.set_color_transfer_characteristic(transfer.into());
            }
            if let Some(space) = settings.colors.space {
                context.set_colorspace(space.into());
            }
        } else {
            context.set_color_primaries(ffmpeg::color::Primaries::BT709);
            context.set_color_transfer_characteristic(ffmpeg::color::TransferCharacteristic::BT709);
            context.set_colorspace(ffmpeg::color::Space::BT709);
        }
        context.set_color_range(ffmpeg::color::Range::MPEG);
        if settings.global_header {
            context.set_flags(Flags::GLOBAL_HEADER);
        }
        if let Some(rate) = settings.frame_rate {
            // SAFETY: the context is valid and not yet open.
            unsafe { (*context.as_mut_ptr()).framerate = rate.into() };
        }

        let mut options = Dictionary::new();
        let main10 = if settings.ten_bit { "main10" } else { "main" };
        let profile = match settings.codec {
            VideoCodec::Hevc => main10,
            _ => "high",
        };
        options.set("profile", profile);
        if settings.encoder.ends_with("_videotoolbox") {
            // Hardware only: a software fallback is too slow to keep up.
            options.set("allow_sw", "0");
        } else if settings.encoder.ends_with("_nvenc") {
            options.set("preset", "p4");
            options.set("forced-idr", "1");
        }

        #[cfg(target_os = "linux")]
        let vaapi = if settings.encoder.ends_with("_vaapi") {
            let frames = vaapi::Frames::new(format, settings.width, settings.height)?;
            // SAFETY: the context is valid and not yet open; it takes its own
            // reference to the frames.
            unsafe {
                let raw = context.as_mut_ptr();
                (*raw).pix_fmt = ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
                (*raw).hw_frames_ctx = ffi::av_buffer_ref(frames.frames);
            }
            Some(frames)
        } else {
            None
        };

        let encoder = context
            .open_as_with(codec, options)
            .map_err(|err| Error::Encode(format!("{}: {err}", settings.encoder)))?;
        Ok(Self {
            encoder,
            format,
            width: settings.width,
            height: settings.height,
            scaler: None,
            #[cfg(target_os = "linux")]
            vaapi,
        })
    }

    pub fn parameters(&self) -> codec::Parameters {
        codec::Parameters::from(&self.encoder)
    }

    pub fn time_base(&self) -> Rational {
        self.encoder.time_base()
    }

    /// A decoded frame at the encoder's size and in its format, NV12 or
    /// P010, to draw on before it is encoded.
    pub fn scale(&mut self, decoded: &frame::Video) -> Result<frame::Video, Error> {
        let source = (decoded.format(), decoded.width(), decoded.height());
        if self
            .scaler
            .as_ref()
            .is_none_or(|(_, input)| *input != source)
        {
            let scaler = scaling::Context::get(
                source.0,
                source.1,
                source.2,
                self.format,
                self.width,
                self.height,
                scaling::Flags::BILINEAR,
            )
            .map_err(|err| Error::Encode(format!("scaling: {err}")))?;
            self.scaler = Some((scaler, source));
        }
        let (scaler, _) = self.scaler.as_mut().expect("the scaler was just made");
        let mut frame = frame::Video::new(self.format, self.width, self.height);
        scaler
            .run(decoded, &mut frame)
            .map_err(|err| Error::Encode(format!("scaling: {err}")))?;
        Ok(frame)
    }

    /// Encode a frame [`scale`](Self::scale) gave at `pts` (in the
    /// encoder's time base); `key` asks for a keyframe, where a segment
    /// starts.
    pub fn encode(&mut self, mut frame: frame::Video, pts: i64, key: bool) -> Result<(), Error> {
        frame.set_pts(Some(pts));
        frame.set_kind(if key {
            picture::Type::I
        } else {
            picture::Type::None
        });

        #[cfg(target_os = "linux")]
        if let Some(vaapi) = &self.vaapi {
            return vaapi.send(&mut self.encoder, &frame);
        }
        self.encoder
            .send_frame(&frame)
            .map_err(|err| Error::Encode(format!("encoding: {err}")))
    }

    pub fn send_eof(&mut self) -> Result<(), Error> {
        self.encoder
            .send_eof()
            .map_err(|err| Error::Encode(format!("encoding: {err}")))
    }

    /// The next encoded packet, if one is ready.
    pub fn receive(&mut self, packet: &mut Packet) -> bool {
        self.encoder.receive_packet(packet).is_ok()
    }
}

pub(crate) struct AudioSettings {
    pub codec: AudioCodec,
    pub channels: u16,
    pub sample_rate: u32,
    pub bit_rate: u64,
    pub global_header: bool,
}

/// How far the sound may drift from its times, in seconds, before silence
/// is put in or sound dropped to bring it back: about a frame of AAC.
const MAX_DRIFT: f64 = 0.02;

/// An audio encoder, resampling what it is given to its format and
/// handing it over a frame of its size at a time.
pub(crate) struct AudioEncoder {
    encoder: ffmpeg::encoder::Audio,
    format: Sample,
    layout: ChannelLayout,
    rate: u32,
    frame_size: usize,
    resampler: Option<(resampling::Context, (Sample, ChannelLayout, u32))>,
    fifo: *mut ffi::AVAudioFifo,
    /// The next frame's timestamp, in samples.
    next_pts: Option<i64>,
    /// Samples still to drop before encoding, to start where asked.
    skip: usize,
}

// SAFETY: the encoder, resampler and FIFO are used by one thread at a time.
unsafe impl Send for AudioEncoder {}

impl Drop for AudioEncoder {
    fn drop(&mut self) {
        // SAFETY: the FIFO was allocated in `open` and is freed once.
        unsafe { ffi::av_audio_fifo_free(self.fifo) };
    }
}

impl AudioEncoder {
    pub fn open(settings: &AudioSettings) -> Result<Self, Error> {
        let name = match settings.codec {
            AudioCodec::Aac => "aac",
            AudioCodec::Ac3 => "ac3",
            AudioCodec::Eac3 => "eac3",
            AudioCodec::Flac => "flac",
            _ => return Err(Error::NoEncoder),
        };
        let codec = ffmpeg::encoder::find_by_name(name).ok_or(Error::NoEncoder)?;
        let format = codec
            .audio()
            .ok()
            .and_then(|audio| audio.formats())
            .and_then(|mut formats| formats.next())
            .unwrap_or(Sample::F32(ffmpeg::format::sample::Type::Planar));
        let layout = ChannelLayout::default(i32::from(settings.channels));
        let mut context = codec::context::Context::new_with_codec(codec)
            .encoder()
            .audio()
            .map_err(|err| Error::Encode(format!("{name}: {err}")))?;
        context.set_rate(settings.sample_rate as i32);
        context.set_channel_layout(layout);
        context.set_format(format);
        context.set_time_base(Rational::new(1, settings.sample_rate as i32));
        context.set_bit_rate(settings.bit_rate as usize);
        if settings.global_header {
            context.set_flags(Flags::GLOBAL_HEADER);
        }
        let encoder = context
            .open_as(codec)
            .map_err(|err| Error::Encode(format!("{name}: {err}")))?;
        let frame_size = (encoder.frame_size() as usize).max(1);
        // SAFETY: a new FIFO, freed on drop.
        let fifo = unsafe {
            ffi::av_audio_fifo_alloc(
                format.into(),
                i32::from(settings.channels),
                frame_size as i32 * 4,
            )
        };
        if fifo.is_null() {
            return Err(Error::Encode("no memory for audio".into()));
        }
        Ok(Self {
            encoder,
            format,
            layout,
            rate: settings.sample_rate,
            frame_size,
            resampler: None,
            fifo,
            next_pts: None,
            skip: 0,
        })
    }

    pub fn parameters(&self) -> codec::Parameters {
        codec::Parameters::from(&self.encoder)
    }

    pub fn time_base(&self) -> Rational {
        Rational::new(1, self.rate as i32)
    }

    /// Take a decoded frame whose first sample plays at `time` seconds;
    /// sound before `start` is dropped.
    pub fn send(&mut self, decoded: &mut frame::Audio, time: f64, start: f64) -> Result<(), Error> {
        if decoded.samples() == 0 {
            return Ok(());
        }
        if self.next_pts.is_none() {
            let begin = time.max(start);
            self.next_pts = Some((begin * f64::from(self.rate)).round() as i64);
            self.skip = ((start - time).max(0.0) * f64::from(self.rate)).round() as usize;
        }
        let mut layout = decoded.channel_layout();
        if layout.is_empty() || layout.channels() != i32::from(decoded.channels()) {
            layout = ChannelLayout::default(i32::from(decoded.channels()));
            decoded.set_channel_layout(layout);
        }
        let input = (decoded.format(), layout, decoded.rate());
        let in_rate = f64::from(input.2.max(1));
        if self
            .resampler
            .as_ref()
            .is_none_or(|(_, current)| *current != input)
        {
            // The resampler keeps the sound where its times put it, padding
            // a gap in it with silence and dropping sound that overlaps
            // what came before, past MAX_DRIFT; one made partway carries on
            // from where the sound sent so far reaches, in its samples.
            let mut options = Dictionary::new();
            options.set("min_comp", "0.001");
            options.set("min_hard_comp", &MAX_DRIFT.to_string());
            let reached = (self.reached() * in_rate).round() as i64;
            options.set("first_pts", &reached.to_string());
            let resampler = resampling::Context::get_with(
                input.0,
                input.1,
                input.2,
                self.format,
                self.layout,
                self.rate,
                options,
            )
            .map_err(|err| Error::Encode(format!("resampling: {err}")))?;
            self.resampler = Some((resampler, input));
        }
        let (resampler, _) = self
            .resampler
            .as_mut()
            .expect("the resampler was just made");
        // SAFETY: the resampler is open; the time is in its units, a
        // second being the input's rate times the output's.
        unsafe {
            ffi::swr_next_pts(
                resampler.as_mut_ptr(),
                (time * in_rate * f64::from(self.rate)).round() as i64,
            )
        };
        let pending = resampler
            .delay()
            .map_or(0, |delay| delay.output.max(0) as usize);
        let capacity =
            decoded.samples() * self.rate as usize / input.2.max(1) as usize + pending + 64;
        let mut resampled = frame::Audio::new(self.format, capacity, self.layout);
        resampler
            .run(decoded, &mut resampled)
            .map_err(|err| Error::Encode(format!("resampling: {err}")))?;
        self.push(&resampled)?;
        self.encode_whole_frames()
    }

    /// Where the sound sent so far reaches, in seconds: the next frame's
    /// time, and what's waiting to be encoded, less what's to be dropped.
    fn reached(&self) -> f64 {
        // SAFETY: the FIFO is valid.
        let waiting = i64::from(unsafe { ffi::av_audio_fifo_size(self.fifo) });
        let samples = self.next_pts.unwrap_or(0) + waiting - self.skip as i64;
        samples as f64 / f64::from(self.rate)
    }

    fn push(&mut self, resampled: &frame::Audio) -> Result<(), Error> {
        let samples = resampled.samples();
        if samples == 0 {
            return Ok(());
        }
        // SAFETY: the frame holds `samples` samples in the FIFO's format and
        // layout; the FIFO grows to take them.
        let written = unsafe {
            ffi::av_audio_fifo_write(
                self.fifo,
                (*resampled.as_ptr()).extended_data.cast(),
                samples as i32,
            )
        };
        if written < samples as i32 {
            return Err(Error::Encode("no memory for audio".into()));
        }
        if self.skip > 0 {
            // SAFETY: the FIFO is valid; draining no more than it holds.
            let drop = unsafe { ffi::av_audio_fifo_size(self.fifo) }.min(self.skip as i32);
            unsafe { ffi::av_audio_fifo_drain(self.fifo, drop) };
            self.skip -= drop as usize;
        }
        Ok(())
    }

    fn encode_whole_frames(&mut self) -> Result<(), Error> {
        // SAFETY: the FIFO is valid.
        while unsafe { ffi::av_audio_fifo_size(self.fifo) } as usize >= self.frame_size {
            self.encode_frame(self.frame_size)?;
        }
        Ok(())
    }

    /// Encode `samples` from the FIFO, padded with silence to a whole frame.
    fn encode_frame(&mut self, samples: usize) -> Result<(), Error> {
        let mut frame = frame::Audio::new(self.format, self.frame_size, self.layout);
        frame.set_rate(self.rate);
        // SAFETY: the frame has room for a whole frame of samples, silenced
        // first so a short last one is padded.
        unsafe {
            ffi::av_samples_set_silence(
                (*frame.as_mut_ptr()).extended_data,
                0,
                self.frame_size as i32,
                i32::from(self.layout.channels() as u16),
                self.format.into(),
            );
            ffi::av_audio_fifo_read(
                self.fifo,
                (*frame.as_mut_ptr()).extended_data.cast(),
                samples as i32,
            );
        }
        let pts = self.next_pts.unwrap_or(0);
        frame.set_pts(Some(pts));
        self.next_pts = Some(pts + self.frame_size as i64);
        self.encoder
            .send_frame(&frame)
            .map_err(|err| Error::Encode(format!("encoding sound: {err}")))
    }

    /// Encode what is left, then end the stream.
    pub fn send_eof(&mut self) -> Result<(), Error> {
        self.encode_whole_frames()?;
        // SAFETY: the FIFO is valid.
        let left = unsafe { ffi::av_audio_fifo_size(self.fifo) }.max(0) as usize;
        if left > 0 && self.next_pts.is_some() {
            self.encode_frame(left)?;
        }
        self.encoder
            .send_eof()
            .map_err(|err| Error::Encode(format!("encoding sound: {err}")))
    }

    pub fn receive(&mut self, packet: &mut Packet) -> bool {
        self.encoder.receive_packet(packet).is_ok()
    }
}

/// VA-API encoders take frames in the GPU's memory: a pool of them, each
/// uploaded from the scaled frame before it is encoded.
#[cfg(target_os = "linux")]
mod vaapi {
    use std::ptr;

    use ffmpeg::ffi;
    use ffmpeg::format::Pixel;
    use ffmpeg::frame;
    use ffmpeg_next as ffmpeg;

    use crate::Error;

    pub struct Frames {
        device: *mut ffi::AVBufferRef,
        pub frames: *mut ffi::AVBufferRef,
    }

    impl Drop for Frames {
        fn drop(&mut self) {
            // SAFETY: both references were taken in `new`, and are released once.
            unsafe {
                ffi::av_buffer_unref(&mut self.frames);
                ffi::av_buffer_unref(&mut self.device);
            }
        }
    }

    impl Frames {
        pub fn new(format: Pixel, width: u32, height: u32) -> Result<Self, Error> {
            // SAFETY: FFmpeg's hardware context API, each result checked
            // before use; what is made is released on drop or failure.
            unsafe {
                let mut device = ptr::null_mut();
                if ffi::av_hwdevice_ctx_create(
                    &mut device,
                    ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                ) < 0
                {
                    return Err(Error::NoEncoder);
                }
                let mut frames = ffi::av_hwframe_ctx_alloc(device);
                if frames.is_null() {
                    ffi::av_buffer_unref(&mut device);
                    return Err(Error::NoEncoder);
                }
                let context = (*frames).data.cast::<ffi::AVHWFramesContext>();
                (*context).format = ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
                (*context).sw_format = format.into();
                (*context).width = width as i32;
                (*context).height = height as i32;
                (*context).initial_pool_size = 20;
                if ffi::av_hwframe_ctx_init(frames) < 0 {
                    ffi::av_buffer_unref(&mut frames);
                    ffi::av_buffer_unref(&mut device);
                    return Err(Error::NoEncoder);
                }
                Ok(Self { device, frames })
            }
        }

        /// Upload `frame` and hand it to the encoder.
        pub fn send(
            &self,
            encoder: &mut ffmpeg::encoder::Video,
            frame: &frame::Video,
        ) -> Result<(), Error> {
            // SAFETY: a frame from the pool, filled from `frame`, given to
            // the encoder (which takes its own reference) and then freed.
            unsafe {
                let mut uploaded = ffi::av_frame_alloc();
                if uploaded.is_null() {
                    return Err(Error::Encode("no memory for a frame".into()));
                }
                let result = if ffi::av_hwframe_get_buffer(self.frames, uploaded, 0) < 0
                    || ffi::av_hwframe_transfer_data(uploaded, frame.as_ptr(), 0) < 0
                {
                    Err(Error::Encode("uploading a frame to VA-API failed".into()))
                } else {
                    (*uploaded).pts = (*frame.as_ptr()).pts;
                    (*uploaded).pict_type = (*frame.as_ptr()).pict_type;
                    if ffi::avcodec_send_frame(encoder.as_mut_ptr(), uploaded) < 0 {
                        Err(Error::Encode("encoding with VA-API failed".into()))
                    } else {
                        Ok(())
                    }
                };
                ffi::av_frame_free(&mut uploaded);
                result
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/encode.rs"]
mod tests;
