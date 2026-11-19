//! Writing packets into fragmented MP4 or MPEG-TS, in memory, cut into
//! segments where the caller says. Each output timestamp is moved later by
//! `OFFSET`, so none is negative (MP4 fragments can't start before zero)
//! and segments written by different jobs line up.

use std::ffi::CString;
use std::ptr;

use ffmpeg::ffi;
use ffmpeg::packet::Mut;
use ffmpeg::{Dictionary, Packet, Rational, codec};
use ffmpeg_next as ffmpeg;

use crate::Error;
use crate::profile::Segments;

/// How much later than the source every output timestamp is, in seconds.
pub(crate) const OFFSET: f64 = 10.0;

/// A stream to write: its codec parameters and the time base its packets'
/// timestamps come in.
pub(crate) struct OutStream {
    pub parameters: codec::Parameters,
    pub time_base: Rational,
}

pub(crate) struct Muxer {
    context: *mut ffi::AVFormatContext,
    kind: Segments,
    /// Each stream's packets' time base, to rescale from.
    input_time_bases: Vec<Rational>,
}

// SAFETY: the muxer is used by one thread at a time, the job's.
unsafe impl Send for Muxer {}

impl Drop for Muxer {
    fn drop(&mut self) {
        // SAFETY: the context and its dynamic buffer were made in `new`
        // and are freed once.
        unsafe {
            self.close_buffer();
            ffi::avformat_free_context(self.context);
        }
    }
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
    pub fn new(kind: Segments, streams: &[OutStream]) -> Result<Self, Error> {
        let format = CString::new(match kind {
            Segments::Fmp4 => "mp4",
            Segments::Ts => "mpegts",
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
                (*parameters).codec_tag = if kind == Segments::Fmp4
                    && (*parameters).codec_id == ffi::AVCodecID::AV_CODEC_ID_HEVC
                {
                    u32::from_le_bytes(*b"hvc1")
                } else {
                    0
                };
                (*out).time_base = stream.time_base.into();
            }
            muxer.open_buffer()?;
            Ok(muxer)
        }
    }

    /// Start writing: the header, which for MP4 is the init segment.
    pub fn write_header(&mut self) -> Result<Vec<u8>, Error> {
        let mut options = Dictionary::new();
        // Timestamps are written as they come: shifting them to start at
        // zero would shift each job's differently.
        options.set("avoid_negative_ts", "disabled");
        match self.kind {
            Segments::Fmp4 => {
                options.set(
                    "movflags",
                    "empty_moov+default_base_moof+frag_custom+frag_discont+skip_trailer",
                );
                options.set("use_editlist", "0");
            }
            Segments::Ts => {}
        }
        // SAFETY: the context is set up with its streams and a buffer.
        unsafe {
            let mut raw = options.disown();
            let result = ffi::avformat_write_header(self.context, &mut raw);
            ffi::av_dict_free(&mut raw);
            check(result, "writing the header")?;
        }
        self.take()
    }

    /// The time base packets of stream `index` are written in.
    pub fn time_base(&self, index: usize) -> Rational {
        // SAFETY: the stream exists; its time base is set by the header.
        unsafe { (**(*self.context).streams.add(index)).time_base.into() }
    }

    /// Write a packet of stream `index`, its timestamps in that stream's
    /// input time base.
    pub fn write(&mut self, mut packet: Packet, index: usize) -> Result<(), Error> {
        let output = self.time_base(index);
        packet.rescale_ts(self.input_time_bases[index], output);
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

    /// End the segment written since the last cut, and return it.
    pub fn cut(&mut self) -> Result<Vec<u8>, Error> {
        // SAFETY: flushing the interleaving queue, then the muxer's own
        // buffers (an MP4 fragment, or MPEG-TS's PES packets).
        unsafe {
            check(
                ffi::av_interleaved_write_frame(self.context, ptr::null_mut()),
                "flushing",
            )?;
            check(
                ffi::av_write_frame(self.context, ptr::null_mut()),
                "flushing",
            )?;
        }
        let segment = self.take()?;
        if self.kind == Segments::Ts {
            // Each segment starts with the tables that say what it holds.
            self.set_option("mpegts_flags", "+resend_headers");
        }
        Ok(segment)
    }

    /// What has been written since the last take, without cutting: for a
    /// stream sent as it is made.
    pub fn take(&mut self) -> Result<Vec<u8>, Error> {
        // SAFETY: closing the buffer hands back what it holds; a new one is
        // opened for what comes next.
        unsafe {
            let bytes = self.close_buffer();
            self.open_buffer()?;
            Ok(bytes)
        }
    }

    /// Write what is left and end the output.
    pub fn finish(&mut self) -> Result<Vec<u8>, Error> {
        let last = self.cut()?;
        // SAFETY: the context is open; the trailer goes to the buffer.
        unsafe { check(ffi::av_write_trailer(self.context), "writing the trailer")? };
        let mut rest = self.take()?;
        let mut all = last;
        all.append(&mut rest);
        Ok(all)
    }

    fn set_option(&mut self, name: &str, value: &str) {
        let name = CString::new(name).expect("no NUL");
        let value = CString::new(value).expect("no NUL");
        // SAFETY: the muxer's private options, by name.
        unsafe {
            ffi::av_opt_set((*self.context).priv_data, name.as_ptr(), value.as_ptr(), 0);
        }
    }

    unsafe fn open_buffer(&mut self) -> Result<(), Error> {
        // SAFETY: the context is valid; its `pb` is ours.
        unsafe {
            check(
                ffi::avio_open_dyn_buf(&mut (*self.context).pb),
                "opening a buffer",
            )
        }
    }

    /// Close the buffer and return what it held.
    unsafe fn close_buffer(&mut self) -> Vec<u8> {
        // SAFETY: `pb` is a dynamic buffer opened by `open_buffer`, or null.
        unsafe {
            let pb = (*self.context).pb;
            if pb.is_null() {
                return Vec::new();
            }
            let mut data = ptr::null_mut();
            let size = ffi::avio_close_dyn_buf(pb, &mut data);
            (*self.context).pb = ptr::null_mut();
            let bytes = if size > 0 && !data.is_null() {
                std::slice::from_raw_parts(data, size as usize).to_vec()
            } else {
                Vec::new()
            };
            ffi::av_free(data.cast());
            bytes
        }
    }
}
