//! Converting decoded frames to BGRA images for an SDR screen: corrected
//! for non-square pixels, scaled down, HDR tone mapped, with the gamma set.

use std::ffi::c_int;
use std::ptr;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow, bail};
use ffmpeg::software::scaling;
use ffmpeg::util::{color, format::Pixel};
use ffmpeg::{Rational, frame};
use ffmpeg_next as ffmpeg;
use gpui::RenderImage;

use super::rational;
use crate::media::{dovi, hdr, picture};
use ffmpeg_safe::Colors;

/// What a frame is converted to before it becomes BGRA.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Light {
    /// Straight to BGRA, by swscale.
    Sdr,
    /// 16-bit RGB, then tone mapped.
    Hdr(hdr::Transfer),
    /// 16-bit 4:4:4 planes, reshaped, then tone mapped.
    DolbyVision,
}

pub(in crate::media) struct Scaler {
    context: scaling::Context,
    key: (Pixel, u32, u32, Light),
    width: u32,
    height: u32,
    output: frame::Video,
    /// For HDR frames, Dolby Vision's included (always PQ).
    tone_mapper: Option<hdr::ToneMapper>,
}

/// Whether frames go to gpui as their planes, for the GPU to convert:
/// unless `PLAYS_GPU_CONVERSION=0`, which converts them on the CPU, as
/// before, to compare the two.
#[cfg(not(target_os = "macos"))]
pub(super) fn gpu_conversion() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PLAYS_GPU_CONVERSION").is_none_or(|v| v != "0"))
}

/// Convert a decoded frame to a BGRA image gpui can draw.
pub(super) fn convert(
    scaler: &mut Option<Scaler>,
    max_short_side: Option<u32>,
    frame: &frame::Video,
) -> Result<Arc<RenderImage>> {
    let (width, height, pixels) = convert_pixels(scaler, max_short_side, frame)?;
    // gpui reads image bytes as BGRA, which is exactly what we converted to.
    let buffer = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| anyhow!("frame buffer has the wrong size"))?;
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// Convert a decoded frame to tightly packed BGRA for an SDR screen, its
/// width and height and its pixels: corrected for non-square pixels,
/// scaled down to `max_short_side`, HDR tone mapped, with the gamma set.
pub(in crate::media) fn convert_pixels(
    scaler: &mut Option<Scaler>,
    max_short_side: Option<u32>,
    frame: &frame::Video,
) -> Result<(u32, u32, Vec<u8>)> {
    let reshaping = dovi::reshaping(frame);
    let light = match (&reshaping, hdr::Transfer::of(frame)) {
        (Some(_), _) => Light::DolbyVision,
        (None, Some(transfer)) => Light::Hdr(transfer),
        (None, None) => Light::Sdr,
    };
    let key = (frame.format(), frame.width(), frame.height(), light);
    if scaler.as_ref().is_none_or(|s| s.key != key) {
        let (width, height) = output_size(
            frame.width(),
            frame.height(),
            frame.aspect_ratio(),
            max_short_side,
        );
        let (format, tone_mapper) = match light {
            Light::Sdr => (Pixel::BGRA, None),
            Light::Hdr(transfer) => (Pixel::RGB48LE, Some(hdr::ToneMapper::new(transfer))),
            Light::DolbyVision => (
                Pixel::YUV444P16LE,
                Some(hdr::ToneMapper::new(hdr::Transfer::Pq)),
            ),
        };
        let mut context = scaling::Context::get(
            key.0,
            key.1,
            key.2,
            format,
            width,
            height,
            scaling::Flags::BILINEAR,
        )
        .context("creating the pixel converter")?;
        // Dolby Vision's samples aren't YCbCr: they are only scaled, never
        // converted.
        if light != Light::DolbyVision {
            set_colorspace(&mut context, frame);
        }
        *scaler = Some(Scaler {
            context,
            key,
            width,
            height,
            output: frame::Video::empty(),
            tone_mapper,
        });
    }
    let scaler = scaler.as_mut().expect("scaler was just created");
    let gamma = picture::gamma(light != Light::Sdr);
    let (width, height) = (scaler.width, scaler.height);
    let Some(mapper) = &mut scaler.tone_mapper else {
        // SDR is scaled straight into the picture's own pixels.
        let mut pixels = scale_to_bgra(&mut scaler.context, frame, width, height)?;
        if gamma != 1.0 {
            let table = picture::gamma_table(gamma);
            picture::in_rows(&mut pixels, width as usize * 4, |_, row| {
                for pixel in row.as_chunks_mut::<4>().0 {
                    let [b, g, r, a] = *pixel;
                    *pixel = [table[b as usize], table[g as usize], table[r as usize], a];
                }
            });
        }
        return Ok((width, height, pixels));
    };
    mapper.set_gamma(gamma);
    mapper.set_peak(match &reshaping {
        Some(reshaping) => reshaping.peak_nits(),
        None => hdr::peak_nits(frame),
    });
    scaler.context.run(frame, &mut scaler.output)?;
    let output = &scaler.output;
    let plane = |index| ffmpeg_safe::plane(output, index).context("a scaled plane is missing");
    let pixels = match &reshaping {
        Some(reshaping) => dovi::render(
            reshaping,
            mapper,
            [plane(0)?, plane(1)?, plane(2)?],
            width as usize,
            height as usize,
        ),
        None => {
            let (data, stride) = plane(0)?;
            mapper.run(data, stride, width as usize, height as usize)
        }
    };
    Ok((width, height, pixels))
}

/// Scale `frame` with `context`, made for its format and size, into
/// `width` x `height` tightly packed BGRA, with no frame in between.
fn scale_to_bgra(
    context: &mut scaling::Context,
    frame: &frame::Video,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let stride = width as usize * 4;
    let len = stride * height as usize;
    let mut pixels = Vec::<u8>::with_capacity(len);
    let planes = [
        pixels.as_mut_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
    ];
    let strides = [c_int::try_from(stride)?, 0, 0, 0];
    // SAFETY: the context reads `frame`'s planes, as long as their strides
    // say, for the format and size it was made for, which its key holds
    // `frame` to; and writes `height` rows of `stride` bytes into the one
    // plane of packed BGRA it was made for, which `pixels` has room for.
    let made = unsafe {
        ffmpeg::ffi::sws_scale(
            context.as_mut_ptr(),
            (*frame.as_ptr()).data.as_ptr() as *const *const u8,
            (*frame.as_ptr()).linesize.as_ptr(),
            0,
            frame.height() as c_int,
            planes.as_ptr(),
            strides.as_ptr(),
        )
    };
    if made != height as c_int {
        bail!("the pixel converter made {made} rows of {height}");
    }
    // SAFETY: each of those rows is `stride` bytes, every pixel written:
    // all of `len`.
    unsafe { pixels.set_len(len) };
    Ok(pixels)
}

/// How many times wider or narrower than square a pixel is taken to be at
/// most: past any anamorphic lens's, so a file can't stretch a frame to
/// hundreds of megabytes.
const MOST_PIXEL_SHAPE: f64 = 16.0;

/// Pixels a frame is stretched to at most: DCI 8K's.
const MOST_PIXELS: f64 = 8192.0 * 4320.0;

/// Size to convert a frame to: corrects non-square pixels and applies the
/// resolution cap to the shorter side, keeping both sides even. No bigger
/// than it was decoded, or than 8K, whatever shape its pixels are.
pub(crate) fn output_size(
    width: u32,
    height: u32,
    sar: Rational,
    max_short_side: Option<u32>,
) -> (u32, u32) {
    let sar = rational(sar).map_or(1.0, |sar| {
        sar.clamp(1.0 / MOST_PIXEL_SHAPE, MOST_PIXEL_SHAPE)
    });
    let mut w = f64::from(width) * sar;
    let mut h = f64::from(height);
    if let Some(max) = max_short_side {
        let short = w.min(h);
        if short > f64::from(max) {
            let scale = f64::from(max) / short;
            w *= scale;
            h *= scale;
        }
    }
    let most = (f64::from(width) * f64::from(height)).max(MOST_PIXELS);
    if w * h > most {
        let scale = (most / (w * h)).sqrt();
        w *= scale;
        h *= scale;
    }
    let even = |v: f64| ((v.round() as u32) & !1).max(2);
    (even(w), even(h))
}

/// Gives `frame`'s pixels the shape `aspect`, where its container gives
/// one.
pub(in crate::media) fn set_aspect(frame: &mut frame::Video, aspect: Option<Rational>) {
    if let Some(aspect) = aspect {
        // SAFETY: the frame is valid and borrowed mutably, and the field a
        // plain pair of numbers.
        unsafe { (*frame.as_mut_ptr()).sample_aspect_ratio = aspect.into() };
    }
}

/// Use the right YUV matrix and range so colors match other players.
pub(in crate::media) fn set_colorspace(context: &mut scaling::Context, frame: &frame::Video) {
    use ffmpeg::ffi;
    let colors = Colors::of_frame(frame);
    let matrix = match colors.space {
        color::Space::BT709 => ffi::SWS_CS_ITU709,
        color::Space::FCC => ffi::SWS_CS_FCC,
        color::Space::SMPTE240M => ffi::SWS_CS_SMPTE240M,
        color::Space::BT2020NCL | color::Space::BT2020CL => ffi::SWS_CS_BT2020,
        color::Space::Unspecified if frame.height() >= 720 => ffi::SWS_CS_ITU709,
        _ => ffi::SWS_CS_DEFAULT,
    };
    let full_range = matches!(colors.range, color::Range::JPEG) as c_int;
    // SAFETY: the context pointer is valid for the lifetime of `context`, and
    // sws_getCoefficients returns pointers to static tables.
    unsafe {
        ffi::sws_setColorspaceDetails(
            context.as_mut_ptr(),
            ffi::sws_getCoefficients(matrix),
            full_range,
            ffi::sws_getCoefficients(ffi::SWS_CS_DEFAULT),
            1,
            0,
            1 << 16,
            1 << 16,
        );
    }
}

#[cfg(test)]
#[path = "tests/convert.rs"]
mod tests;
