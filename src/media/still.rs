//! Stills: a frame of a video decoded again on its own, at its full size,
//! for a screenshot. A player decodes ahead of the playhead, maybe on the
//! GPU and maybe scaled down to the resolution limit, and keeps only what
//! it shows; this opens the file again, finds the frame by its timestamp
//! and converts it on the CPU, however the player decoded it. A live
//! stream can't be opened again where it was: its still is the picture the
//! player shows, read back from the GPU if it's there.

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use ffmpeg::software::scaling;
use ffmpeg::util::format::Pixel;
use ffmpeg::{Packet, frame};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;

use super::colors::Colors;
use super::decoder::{convert_pixels, open_input, open_video, output_size, set_colorspace};
use super::dovi::Reshaping;
use super::hdr::{self, ToneMapper, Transfer};
use super::picture::{Picture, rows};
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
    of_frame(&frame, keep_hdr)
}

/// The picture a player shows, as it shows it, at the size it plays at:
/// for a live stream, which can't be opened again where it was. HDR video
/// is tone mapped as `grab` does, unless `keep_hdr`; a picture in memory
/// already was.
pub fn shown(picture: &Picture, keep_hdr: bool) -> Result<Still> {
    let frame = match picture {
        Picture::Image(image) => return in_memory(image),
        #[cfg(target_os = "macos")]
        Picture::Surface(surface) => {
            let frame = surface.read()?;
            if let Some(reshaping) = surface.reshaping() {
                return reshaped(&frame, reshaping, keep_hdr);
            }
            frame
        }
        #[cfg(not(target_os = "macos"))]
        Picture::Yuv(yuv) => yuv.read()?,
    };
    of_frame(&frame, keep_hdr)
}

/// A frame of `format` samples, `size` in pixels, copied from `planes`
/// (each its samples and the bytes from one row to the next) and tagged
/// with `colors`.
pub(super) fn frame_of(
    format: Pixel,
    (width, height): (u32, u32),
    planes: &[(&[u8], usize)],
    colors: Colors,
) -> Result<frame::Video> {
    let mut frame = frame::Video::new(format, width, height);
    ensure!(
        frame.planes() == planes.len(),
        "{format:?} has {} planes, not {}",
        frame.planes(),
        planes.len()
    );
    for (index, &(samples, stride)) in planes.iter().enumerate() {
        let rows = frame.plane_height(index) as usize;
        ensure!(samples.len() >= rows * stride, "plane {index} is short");
        let into_stride = frame.stride(index);
        let row = stride.min(into_stride);
        let into = frame.data_mut(index);
        for y in 0..rows {
            into[y * into_stride..][..row].copy_from_slice(&samples[y * stride..][..row]);
        }
    }
    frame.set_color_range(colors.range);
    frame.set_color_space(colors.space);
    frame.set_color_primaries(colors.primaries);
    frame.set_color_transfer_characteristic(colors.transfer);
    Ok(frame)
}

/// `frame` as a still: tone mapped for an SDR screen, unless `keep_hdr`.
fn of_frame(frame: &frame::Video, keep_hdr: bool) -> Result<Still> {
    let transfer = Transfer::of(frame);
    let reshaping = Reshaping::of(frame);
    match (keep_hdr, reshaping, transfer) {
        (true, Some(reshaping), _) => dolby_vision(frame, &reshaping),
        (true, None, Some(transfer)) => hdr(frame, transfer),
        _ => sdr(frame),
    }
}

/// A picture in memory, BGRA, as a player converted it for an SDR screen.
fn in_memory(image: &RenderImage) -> Result<Still> {
    let size = image.size(0);
    let (width, height) = (size.width.0.max(0) as u32, size.height.0.max(0) as u32);
    let bgra = image.as_bytes(0).context("the picture has no pixels")?;
    ensure!(
        bgra.len() == width as usize * height as usize * 4,
        "the picture has the wrong size"
    );
    Ok(from_bgra(width, height, bgra))
}

/// Tightly packed BGRA, as sRGB.
fn from_bgra(width: u32, height: u32, bgra: &[u8]) -> Still {
    let (bgra, _) = bgra.as_chunks::<4>();
    let pixels = bgra
        .iter()
        .map(|&[b, g, r, _]| [r, g, b].map(|c| u16::from(c) * 257))
        .collect();
    Still {
        width,
        height,
        pixels,
        hdr: None,
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
    Ok(from_bgra(width, height, &bgra))
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

/// Dolby Vision read back as decoded, its reshaping apart: kept, or tone
/// mapped for an SDR screen as a player shows it.
#[cfg(target_os = "macos")]
fn reshaped(frame: &frame::Video, reshaping: &Reshaping, keep_hdr: bool) -> Result<Still> {
    let kept = dolby_vision(frame, reshaping)?;
    if keep_hdr {
        return Ok(kept);
    }
    let mut mapper = ToneMapper::new(Transfer::Pq);
    mapper.set_gamma(super::picture::gamma(true));
    mapper.set_peak(reshaping.peak_nits());
    let rgb48: Vec<u8> = kept
        .pixels
        .iter()
        .flatten()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    let (width, height) = (kept.width as usize, kept.height as usize);
    let bgra = mapper.run(&rgb48, width * 6, width, height);
    Ok(from_bgra(kept.width, kept.height, &bgra))
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

#[cfg(test)]
#[path = "tests/still.rs"]
mod tests;
