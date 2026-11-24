//! Stills: a frame of a video decoded again on its own, at its full size,
//! for a screenshot. A player decodes ahead of the playhead, maybe on the
//! GPU and maybe scaled down to the resolution limit, and keeps only what
//! it shows; this opens the file again, finds the frame by its timestamp
//! and converts it on the CPU, however the player decoded it.

use anyhow::{Context as _, Result, anyhow, bail};
use ffmpeg::software::scaling;
use ffmpeg::util::format::Pixel;
use ffmpeg::{Packet, frame};
use ffmpeg_next as ffmpeg;

use super::decoder::{convert_pixels, open_input, open_video, output_size, set_colorspace};
use super::dovi::Reshaping;
use super::hdr::{self, ToneMapper, Transfer};
use super::picture::rows;
use super::source::Source;

/// Seeking lands on a keyframe before the frame; if a file's index puts it
/// after, try again this much earlier, in seconds.
const SEEK_BACK: [f64; 3] = [0.0, 5.0, 30.0];

/// A picture in memory.
pub struct Still {
    pub width: u32,
    pub height: u32,
    /// Red, green and blue, 16 bits each, row after row.
    pub pixels: Vec<[u16; 3]>,
    /// How the samples encode light: `None` for sRGB, otherwise BT.2020
    /// with this transfer.
    pub hdr: Option<Transfer>,
}

/// The frame of `stream` at `timestamp` (in the stream's time base), at
/// its display size (corrected for non-square pixels). HDR video is tone
/// mapped for an SDR screen as players show it, unless `keep_hdr`. A stream
/// is opened as it plays, with the resolution limit it opened with,
/// `max_short_side`: its HLS playlist cut down to the same variant.
pub fn grab(
    source: &Source,
    max_short_side: Option<u32>,
    stream: usize,
    timestamp: i64,
    keep_hdr: bool,
) -> Result<Still> {
    let frame = decode(source, max_short_side, stream, timestamp)?;
    let transfer = Transfer::of(&frame);
    let reshaping = Reshaping::of(&frame);
    match (keep_hdr, reshaping, transfer) {
        (true, Some(reshaping), _) => dolby_vision(&frame, &reshaping),
        (true, None, Some(transfer)) => hdr(&frame, transfer),
        _ => sdr(&frame),
    }
}

/// Decode the frame at `timestamp`, or the nearest one.
fn decode(
    source: &Source,
    max_short_side: Option<u32>,
    stream: usize,
    timestamp: i64,
) -> Result<frame::Video> {
    let mut input = open_input(source, max_short_side, || false)?;
    let (parameters, time_base) = {
        let found = input
            .stream(stream)
            .ok_or_else(|| anyhow!("the file no longer has this video track"))?;
        (found.parameters(), f64::from(found.time_base()))
    };
    let (mut decoder, _) = open_video(&parameters, false)?;
    for (attempt, back) in SEEK_BACK.into_iter().enumerate() {
        let micros = ((timestamp as f64 * time_base - back) * 1_000_000.0) as i64;
        input
            .seek(micros, ..micros)
            .context("seeking to the frame")?;
        decoder.flush();
        let last = attempt + 1 == SEEK_BACK.len();
        if let Some(frame) = find(&mut input, &mut decoder, stream, timestamp, last)? {
            return Ok(frame);
        }
    }
    bail!("the frame can't be found in the file")
}

/// Decode from where `input` is until the frame at `timestamp`. `None` if
/// the first frame is already past it (seeking landed too late), unless
/// `or_later`, which takes it then.
fn find(
    input: &mut ffmpeg::format::context::Input,
    decoder: &mut ffmpeg::decoder::Video,
    stream: usize,
    timestamp: i64,
    or_later: bool,
) -> Result<Option<frame::Video>> {
    let mut frame = frame::Video::empty();
    // The last frame before `timestamp`.
    let mut before: Option<frame::Video> = None;
    let mut packet = Packet::empty();
    let mut ended = false;
    loop {
        while decoder.receive_frame(&mut frame).is_ok() {
            match frame.timestamp() {
                Some(ts) if ts < timestamp => {
                    std::mem::swap(before.get_or_insert_with(frame::Video::empty), &mut frame);
                }
                Some(ts) if ts > timestamp => {
                    let Some(before) = before else {
                        return Ok(or_later.then_some(frame));
                    };
                    // Not exactly there: whichever is nearer.
                    let gap = before.timestamp().map_or(i64::MAX, |b| timestamp - b);
                    return Ok(Some(if gap < ts - timestamp { before } else { frame }));
                }
                _ => return Ok(Some(frame)),
            }
        }
        if ended {
            return Ok(before);
        }
        match packet.read(input) {
            Ok(()) if packet.stream() == stream => {
                if let Err(err) = decoder.send_packet(&packet) {
                    log::debug!("screenshot decode error: {err}");
                }
            }
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => {
                let _ = decoder.send_eof();
                ended = true;
            }
            Err(err) => return Err(err).context("reading the file"),
        }
    }
}

/// As a player shows it on an SDR screen.
fn sdr(frame: &frame::Video) -> Result<Still> {
    let (width, height, bgra) = convert_pixels(&mut None, None, frame)?;
    let (bgra, _) = bgra.as_chunks::<4>();
    let pixels = bgra
        .iter()
        .map(|&[b, g, r, _]| [r, g, b].map(|c| u16::from(c) * 257))
        .collect();
    Ok(Still {
        width,
        height,
        pixels,
        hdr: None,
    })
}

/// HDR10 or HLG as it is: BT.2020 RGB, still PQ or HLG.
fn hdr(frame: &frame::Video, transfer: Transfer) -> Result<Still> {
    let (output, width, height) = scale(frame, Pixel::RGB48LE, true)?;
    let (data, stride) = (output.data(0), output.stride(0));
    let pixels = rows(width as usize, height as usize, |y, out| {
        let row = &data[y * stride..][..out.len() * 6];
        let (row, _) = row.as_chunks::<6>();
        for (pixel, out) in row.iter().zip(out) {
            *out = [0, 1, 2].map(|i| u16::from_le_bytes([pixel[2 * i], pixel[2 * i + 1]]));
        }
    });
    Ok(Still {
        width,
        height,
        pixels,
        hdr: Some(transfer),
    })
}

/// Dolby Vision profile 5, reshaped into BT.2020 light and then PQ.
fn dolby_vision(frame: &frame::Video, reshaping: &Reshaping) -> Result<Still> {
    // Dolby Vision's samples aren't YCbCr: they are only scaled.
    let (output, width, height) = scale(frame, Pixel::YUV444P16LE, false)?;
    let planes = [0, 1, 2].map(|plane| (output.data(plane), output.stride(plane)));
    let pq = ToneMapper::new(Transfer::Pq);
    let pixels = rows(width as usize, height as usize, |y, out| {
        let rows = planes.map(|(data, stride)| &data[y * stride..][..out.len() * 2]);
        for (x, out) in out.iter_mut().enumerate() {
            let samples = rows
                .map(|row| f32::from(u16::from_le_bytes([row[2 * x], row[2 * x + 1]])) / 65536.0);
            let lms = reshaping.lms_signal(samples).map(|s| pq.pq_light(s));
            let signal = hdr::encode(reshaping.lms_to_rgb(lms), Transfer::Pq);
            *out = signal.map(|s| (s.clamp(0.0, 1.0) * 65535.0).round() as u16);
        }
    });
    Ok(Still {
        width,
        height,
        pixels,
        hdr: Some(Transfer::Pq),
    })
}

/// The frame in `format` at its display size, converted from YCbCr with
/// its own matrix and range if `colorspace`; scaled as `convert_pixels`
/// does, so kept HDR matches what is tone mapped.
fn scale(
    frame: &frame::Video,
    format: Pixel,
    colorspace: bool,
) -> Result<(frame::Video, u32, u32)> {
    let (width, height) = output_size(frame.width(), frame.height(), frame.aspect_ratio(), None);
    let mut context = scaling::Context::get(
        frame.format(),
        frame.width(),
        frame.height(),
        format,
        width,
        height,
        scaling::Flags::BILINEAR,
    )
    .context("creating the pixel converter")?;
    if colorspace {
        set_colorspace(&mut context, frame);
    }
    let mut output = frame::Video::empty();
    context.run(frame, &mut output)?;
    Ok((output, width, height))
}
