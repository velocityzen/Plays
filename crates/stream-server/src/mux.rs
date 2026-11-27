//! Writing packets into fragmented MP4, MPEG-TS or ADTS, in memory of
//! our own, cut into segments where the caller says and handed over as
//! they are. Each output timestamp is moved later by
//! `OFFSET`, so none is negative (MP4 fragments can't start before zero)
//! and segments written by different jobs line up.

use std::ffi::{CString, c_int, c_void};
use std::{ptr, slice};

use ffmpeg::ffi;
use ffmpeg::packet::Mut;
use ffmpeg::{Dictionary, Packet, Rational, codec};
use ffmpeg_next as ffmpeg;

use crate::Error;
use crate::profile::{AudioCodec, Segments};

/// How much later than the source every output timestamp is, in seconds.
pub(crate) const OFFSET: f64 = 10.0;
/// How much FFmpeg gathers before handing it to the sink.
const IO_BUFFER: usize = 32 * 1024;
const SEEK_SET: c_int = 0;
const SEEK_CUR: c_int = 1;
const SEEK_END: c_int = 2;

/// What the muxer writes: HLS's segments, or the sound alone as internet
/// radio sends it, AAC with ADTS headers or FLAC.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Format {
    Fmp4,
    Ts,
    Adts,
    Flac,
}

impl From<Segments> for Format {
    fn from(segments: Segments) -> Self {
        match segments {
            Segments::Fmp4 => Self::Fmp4,
            Segments::Ts => Self::Ts,
        }
    }
}

impl Format {
    /// Whether it can carry the audio codec as it is.
    pub(crate) fn carries_audio(self, codec: AudioCodec) -> bool {
        match self {
            Self::Fmp4 => Segments::Fmp4.carries_audio(codec),
            Self::Ts => Segments::Ts.carries_audio(codec),
            Self::Adts => codec == AudioCodec::Aac,
            Self::Flac => codec == AudioCodec::Flac,
        }
    }
}

/// A stream to write: its codec parameters and the time base its packets'
/// timestamps come in.
pub(crate) struct OutStream {
    pub parameters: codec::Parameters,
    pub time_base: Rational,
}

pub(crate) struct Muxer {
    context: *mut ffi::AVFormatContext,
    kind: Format,
    /// Each stream's packets' time base, to rescale from.
    input_time_bases: Vec<Rational>,
    /// Where the context's `pb` writes, from `Box::into_raw`.
    sink: *mut Sink,
    /// Whether MP4's init segment is yet to be written. It is written with
    /// the first fragment: only a packet of AC-3 or E-AC-3 sound says what
    /// the sound is.
    init_unwritten: bool,
    /// The init segment, once written, till it is taken.
    init: Option<Vec<u8>>,
}

// SAFETY: the muxer is used by one thread at a time, the job's, and the
// sink only by the muxer.
unsafe impl Send for Muxer {}

impl Drop for Muxer {
    fn drop(&mut self) {
        // SAFETY: the context, its `pb` and the sink were made in `new` and
        // are freed once, the sink after what writes to it.
        unsafe {
            let mut pb = (*self.context).pb;
            (*self.context).pb = ptr::null_mut();
            ffi::avformat_free_context(self.context);
            if !pb.is_null() {
                ffi::av_freep((&raw mut (*pb).buffer).cast());
                ffi::avio_context_free(&mut pb);
            }
            drop(Box::from_raw(self.sink));
        }
    }
}

/// Where the muxer writes: memory of ours, so what was written is handed
/// over as it is instead of copied out of FFmpeg's.
#[derive(Default)]
struct Sink {
    /// What was written since the last take.
    bytes: Vec<u8>,
    /// Where `bytes` starts in all that was written.
    base: u64,
    /// Where the next write goes.
    at: u64,
}

impl Sink {
    /// Write `data` at `at`, over what is there and past it; not before
    /// what was taken.
    fn write(&mut self, data: &[u8]) -> bool {
        let Some(offset) = self
            .at
            .checked_sub(self.base)
            .and_then(|offset| usize::try_from(offset).ok())
        else {
            return false;
        };
        if offset > self.bytes.len() {
            self.bytes.resize(offset, 0);
        }
        let over = (self.bytes.len() - offset).min(data.len());
        self.bytes[offset..offset + over].copy_from_slice(&data[..over]);
        self.bytes.extend_from_slice(&data[over..]);
        self.at += data.len() as u64;
        true
    }

    /// Where the next write goes, as FFmpeg asks; not back into what was
    /// taken.
    fn seek(&mut self, offset: i64, whence: c_int) -> i64 {
        let end = self.base + self.bytes.len() as u64;
        let to = match whence & !ffi::AVSEEK_FORCE {
            ffi::AVSEEK_SIZE => return end as i64,
            SEEK_SET => Some(offset),
            SEEK_CUR => (self.at as i64).checked_add(offset),
            SEEK_END => (end as i64).checked_add(offset),
            _ => return i64::from(ffi::AVERROR(ffi::EINVAL)),
        };
        match to.and_then(|to| u64::try_from(to).ok()) {
            Some(to) if to >= self.base => {
                self.at = to;
                to as i64
            }
            _ => i64::from(ffi::AVERROR(ffi::EIO)),
        }
    }

    /// What was written since the last take.
    fn take(&mut self) -> Vec<u8> {
        let bytes = std::mem::take(&mut self.bytes);
        self.base += bytes.len() as u64;
        bytes
    }
}

unsafe extern "C" fn write_packet(opaque: *mut c_void, data: *const u8, size: c_int) -> c_int {
    // SAFETY: `opaque` is the muxer's sink, which outlives the `pb` calling
    // this and is touched only from inside the muxer's calls into FFmpeg;
    // `data` holds `size` bytes.
    let (sink, data) = unsafe {
        let data = match usize::try_from(size) {
            Ok(size) if size > 0 && !data.is_null() => slice::from_raw_parts(data, size),
            _ => &[],
        };
        (&mut *opaque.cast::<Sink>(), data)
    };
    if sink.write(data) {
        size
    } else {
        ffi::AVERROR(ffi::EIO)
    }
}

unsafe extern "C" fn seek(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    // SAFETY: as in `write_packet`.
    let sink = unsafe { &mut *opaque.cast::<Sink>() };
    sink.seek(offset, whence)
}

fn check(result: i32, what: &str) -> Result<(), Error> {
    if result < 0 {
        Err(Error::Mux(format!(
            "{what}: {}",
            ffmpeg::Error::from(result)
        )))
    } else {
        Ok(())
    }
}

impl Muxer {
    pub fn new(kind: Format, streams: &[OutStream]) -> Result<Self, Error> {
        let format = CString::new(match kind {
            Format::Fmp4 => "mp4",
            Format::Ts => "mpegts",
            Format::Adts => "adts",
            Format::Flac => "flac",
        })
        .expect("no NUL");
        let mut context = ptr::null_mut();
        // SAFETY: FFmpeg's muxing API; every result is checked, and the
        // context is owned by the muxer from here on.
        unsafe {
            check(
                ffi::avformat_alloc_output_context2(
                    &mut context,
                    ptr::null(),
                    format.as_ptr(),
                    ptr::null(),
                ),
                "making the muxer",
            )?;
            let mut muxer = Self {
                context,
                kind,
                input_time_bases: streams.iter().map(|stream| stream.time_base).collect(),
                sink: Box::into_raw(Box::default()),
                init_unwritten: kind == Format::Fmp4,
                init: None,
            };
            for stream in streams {
                let out = ffi::avformat_new_stream(context, ptr::null());
                if out.is_null() {
                    return Err(Error::Mux("adding a stream".into()));
                }
                check(
                    ffi::avcodec_parameters_copy((*out).codecpar, stream.parameters.as_ptr()),
                    "copying a stream's parameters",
                )?;
                let parameters = (*out).codecpar;
                // The source container's tag may mean nothing here; Apple's
                // players want HEVC in MP4 tagged `hvc1`.
                (*parameters).codec_tag = if kind == Format::Fmp4
                    && (*parameters).codec_id == ffi::AVCodecID::AV_CODEC_ID_HEVC
                {
                    u32::from_le_bytes(*b"hvc1")
                } else {
                    0
                };
                (*out).time_base = stream.time_base.into();
            }
            muxer.open_sink()?;
            Ok(muxer)
        }
    }

    /// Start writing: the header. MP4's, the init segment, waits for the
    /// first fragment (`take_init`); the others' starts what is taken next.
    pub fn write_header(&mut self) -> Result<(), Error> {
        let mut options = Dictionary::new();
        // Timestamps are written as they come: shifting them to start at
        // zero would shift each job's differently.
        options.set("avoid_negative_ts", "disabled");
        match self.kind {
            Format::Fmp4 => {
                options.set(
                    "movflags",
                    "empty_moov+delay_moov+default_base_moof+frag_custom+frag_discont+skip_trailer",
                );
                options.set("use_editlist", "0");
            }
            Format::Ts | Format::Adts | Format::Flac => {}
        }
        // SAFETY: the context is set up with its streams and a buffer.
        unsafe {
            let mut raw = options.disown();
            let result = ffi::avformat_write_header(self.context, &mut raw);
            ffi::av_dict_free(&mut raw);
            check(result, "writing the header")
        }
    }

    /// MP4's init segment, once the first cut or `finish` wrote it; then
    /// nothing.
    pub fn take_init(&mut self) -> Option<Vec<u8>> {
        self.init.take()
    }

    /// The time base packets of stream `index` are written in, if it has
    /// one by that number.
    fn time_base(&self, index: usize) -> Option<Rational> {
        // SAFETY: the context is open, its streams' array holding
        // `nb_streams` of them; a stream's time base is set by the header.
        unsafe {
            let context = &*self.context;
            if index >= context.nb_streams as usize {
                return None;
            }
            Some((**context.streams.add(index)).time_base.into())
        }
    }

    /// Write a packet of stream `index`, its timestamps in that stream's
    /// input time base.
    pub fn write(&mut self, mut packet: Packet, index: usize) -> Result<(), Error> {
        let (Some(output), Some(&input)) =
            (self.time_base(index), self.input_time_bases.get(index))
        else {
            return Err(Error::Mux(format!("writing a packet: no stream {index}")));
        };
        packet.rescale_ts(input, output);
        let offset = (OFFSET * f64::from(output.denominator()) / f64::from(output.numerator()))
            .round() as i64;
        packet.set_pts(packet.pts().map(|pts| pts + offset));
        packet.set_dts(packet.dts().map(|dts| dts + offset));
        packet.set_stream(index);
        packet.set_position(-1);
        // SAFETY: the muxer takes the packet's reference and leaves it empty.
        unsafe {
            check(
                ffi::av_interleaved_write_frame(self.context, packet.as_mut_ptr()),
                "writing a packet",
            )
        }
    }

    /// Write out the interleaving queue, then the muxer's own buffers (an
    /// MP4 fragment, or MPEG-TS's PES packets). MP4's first flush writes
    /// the init segment alone, kept apart; a second, the fragment.
    fn flush(&mut self) -> Result<(), Error> {
        // SAFETY: the context is open; flushing writes to the sink.
        unsafe {
            check(
                ffi::av_interleaved_write_frame(self.context, ptr::null_mut()),
                "flushing",
            )?;
        }
        self.flush_fragment()?;
        if self.init_unwritten {
            self.init = Some(self.take()?);
            self.init_unwritten = false;
            self.flush_fragment()?;
        }
        Ok(())
    }

    fn flush_fragment(&mut self) -> Result<(), Error> {
        // SAFETY: the context is open; flushing writes to the sink.
        unsafe {
            check(
                ffi::av_write_frame(self.context, ptr::null_mut()),
                "flushing",
            )
        }
    }

    /// End the segment written since the last cut, and return it.
    pub fn cut(&mut self) -> Result<Vec<u8>, Error> {
        self.flush()?;
        let segment = self.take()?;
        if self.kind == Format::Ts {
            // Each segment starts with the tables that say what it holds.
            self.set_option("mpegts_flags", "+resend_headers");
        }
        Ok(segment)
    }

    /// What has been written since the last take, without cutting: for a
    /// stream sent as it is made.
    pub fn take(&mut self) -> Result<Vec<u8>, Error> {
        // SAFETY: the context's `pb` writes to the sink; flushing it hands
        // the sink what FFmpeg still holds, and nothing writes meanwhile.
        unsafe {
            let pb = (*self.context).pb;
            ffi::avio_flush(pb);
            check((*pb).error, "writing")?;
            Ok((*self.sink).take())
        }
    }

    /// Write what is left and end the output.
    pub fn finish(&mut self) -> Result<Vec<u8>, Error> {
        self.flush()?;
        // SAFETY: the context is open; the trailer goes to the sink.
        unsafe { check(ffi::av_write_trailer(self.context), "writing the trailer")? };
        self.take()
    }

    fn set_option(&mut self, name: &str, value: &str) {
        let name = CString::new(name).expect("no NUL");
        let value = CString::new(value).expect("no NUL");
        // SAFETY: the muxer's private options, by name.
        unsafe {
            ffi::av_opt_set((*self.context).priv_data, name.as_ptr(), value.as_ptr(), 0);
        }
    }

    /// Have the context write to the sink.
    unsafe fn open_sink(&mut self) -> Result<(), Error> {
        // SAFETY: the context is valid and has no `pb` yet; the buffer is
        // FFmpeg's to grow or replace, and freed in `drop`.
        unsafe {
            let buffer = ffi::av_malloc(IO_BUFFER).cast::<u8>();
            if buffer.is_null() {
                return Err(Error::Mux("making a buffer".into()));
            }
            let pb = ffi::avio_alloc_context(
                buffer,
                IO_BUFFER as c_int,
                1,
                self.sink.cast(),
                None,
                Some(write_packet),
                Some(seek),
            );
            if pb.is_null() {
                ffi::av_free(buffer.cast());
                return Err(Error::Mux("making the output".into()));
            }
            // What FLAC's trailer would go back to put its header right in
            // was handed over already.
            if self.kind == Format::Flac {
                (*pb).seekable = 0;
            }
            (*self.context).pb = pb;
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "tests/mux.rs"]
mod tests;
