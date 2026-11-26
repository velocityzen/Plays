//! Frames handed to gpui as their Y'CbCr planes, for the GPU to convert as
//! it draws them (Linux; macOS draws VideoToolbox surfaces). The CPU only
//! copies the planes, or scales them when the frame is bigger than the
//! resolution limit or its pixels aren't square, rather than converting
//! every pixel to RGB and tone mapping HDR as `decoder::convert` does.

use anyhow::{Context as _, Result};
use ffmpeg::frame;
use ffmpeg::software::scaling;
use ffmpeg::util::format::Pixel;
use ffmpeg::util::{chroma, color};
use ffmpeg_next as ffmpeg;
use gpui::{YuvColor, YuvFrame, YuvLayout, YuvTransfer};

use super::colors::Colors;
use super::decoder::output_size;
use super::{dovi, hdr, still};

/// A frame's planes and the size it shows at.
#[derive(Clone, Debug)]
pub struct YuvPicture {
    pub frame: YuvFrame,
    /// Whether it is HDR, for which gamma to draw it with.
    pub hdr: bool,
    /// How its samples are coded, its range as the planes have it.
    pub colors: Colors,
}

impl YuvPicture {
    pub fn size(&self) -> (u32, u32) {
        self.frame.size()
    }

    /// Its planes in a frame tagged with its colors: for a screenshot of
    /// what shows.
    pub fn read(&self) -> Result<frame::Video> {
        let wide = self.frame.is_wide();
        let format = match (self.frame.layout(), wide) {
            (YuvLayout::Planar, false) => Pixel::YUV420P,
            (YuvLayout::SemiPlanar, false) => Pixel::NV12,
            (YuvLayout::Planar, true) => Pixel::YUV420P10LE,
            (YuvLayout::SemiPlanar, true) => Pixel::P010LE,
        };
        let sample_bytes = if wide { 2 } else { 1 };
        let planes: Vec<(&[u8], usize)> = self
            .frame
            .planes()
            .iter()
            .zip(self.frame.plane_sizes())
            .map(|(plane, (width, _, channels))| {
                (plane.as_slice(), (width * channels) as usize * sample_bytes)
            })
            .collect();
        still::frame_of(format, self.size(), &planes, self.colors)
    }
}

/// How the samples of the formats handed over as they are lie.
struct Samples {
    layout: YuvLayout,
    /// 16 bits a sample, rather than 8.
    wide: bool,
    /// Bits a sample's code has.
    depth: u32,
    /// What a sample reads as on the GPU, as a fraction of the largest its
    /// size holds, times this, is its code.
    to_code: f32,
}

fn samples(format: Pixel) -> Option<Samples> {
    let (layout, wide, depth, to_code) = match format {
        Pixel::YUV420P | Pixel::YUVJ420P => (YuvLayout::Planar, false, 8, 255.),
        Pixel::NV12 => (YuvLayout::SemiPlanar, false, 8, 255.),
        Pixel::YUV420P10LE => (YuvLayout::Planar, true, 10, 65535.),
        // P010's 10 bits are the top ones of each 16.
        Pixel::P010LE => (YuvLayout::SemiPlanar, true, 10, 65535. / 64.),
        _ => return None,
    };
    Some(Samples {
        layout,
        wide,
        depth,
        to_code,
    })
}

/// Turns decoded frames into planes for gpui, scaling them when they must
/// be.
#[derive(Default)]
pub struct YuvConverter {
    scaler: Option<YuvScaler>,
}

struct YuvScaler {
    key: (Pixel, u32, u32),
    context: scaling::Context,
    output: frame::Video,
}

impl YuvConverter {
    /// `frame`'s planes, at its display size capped to `max_short_side`;
    /// `None` for frames the GPU doesn't convert: other formats, and Dolby
    /// Vision, whose samples need each frame's reshaping.
    pub fn convert(
        &mut self,
        frame: &frame::Video,
        max_short_side: Option<u32>,
    ) -> Option<Result<YuvPicture>> {
        if dovi::Reshaping::of(frame).is_some() {
            return None;
        }
        samples(frame.format())?;
        let size = output_size(
            frame.width(),
            frame.height(),
            frame.aspect_ratio(),
            max_short_side,
        );
        let at_size = size == (frame.width(), frame.height());
        if at_size && !bottom_up(frame) {
            return Some(planes(frame, frame, chroma_siting(frame)));
        }
        // swscale takes chroma to sit between luma samples, in the frames
        // it reads and in those it makes, but leaves it where it was in a
        // frame it only turns the right way up.
        let siting = if at_size {
            chroma_siting(frame)
        } else {
            [1., 1.]
        };
        Some(
            self.scaled(frame, size)
                .and_then(|scaled| planes(scaled, frame, siting)),
        )
    }

    /// `frame` at `size`, in a planar format of its depth. Its range stays
    /// as it is: swscale only changes it between `yuvj` formats and others.
    fn scaled(&mut self, frame: &frame::Video, size: (u32, u32)) -> Result<&frame::Video> {
        let format = match (frame.format(), samples(frame.format())) {
            (_, Some(Samples { depth: 10, .. })) => Pixel::YUV420P10LE,
            (Pixel::YUVJ420P, _) => Pixel::YUVJ420P,
            _ => Pixel::YUV420P,
        };
        let key = (frame.format(), frame.width(), frame.height());
        if self
            .scaler
            .as_ref()
            .is_none_or(|s| s.key != key || (s.output.width(), s.output.height()) != size)
        {
            let context = scaling::Context::get(
                key.0,
                key.1,
                key.2,
                format,
                size.0,
                size.1,
                scaling::Flags::BILINEAR,
            )
            .context("creating the frame scaler")?;
            self.scaler = Some(YuvScaler {
                key,
                context,
                output: frame::Video::empty(),
            });
        }
        let scaler = self.scaler.as_mut().expect("the scaler was just made");
        scaler.context.run(frame, &mut scaler.output)?;
        Ok(&scaler.output)
    }
}

/// Whether `frame` is stored bottom up, its planes' rows going back through
/// memory (a negative linesize), as Dxtory's flipped frames are: `planes`
/// can't slice those, so swscale turns them the right way up.
fn bottom_up(frame: &frame::Video) -> bool {
    (0..frame.planes()).any(|plane| frame.stride(plane).cast_signed() < 0)
}

/// `samples`' planes, copied row by row, with the colors `tags` says
/// (`samples` is `tags` scaled, or `tags` itself) and their chroma samples
/// where `chroma_siting` says.
fn planes(
    samples_frame: &frame::Video,
    tags: &frame::Video,
    chroma_siting: [f32; 2],
) -> Result<YuvPicture> {
    let format = samples(samples_frame.format()).context("not a format the GPU converts")?;
    let (width, height) = (samples_frame.width(), samples_frame.height());
    let sample_bytes = if format.wide { 2 } else { 1 };
    let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
    let sizes: &[(u32, u32, u32)] = match format.layout {
        YuvLayout::Planar => &[(width, height, 1), (cw, ch, 1), (cw, ch, 1)],
        YuvLayout::SemiPlanar => &[(width, height, 1), (cw, ch, 2)],
    };
    let planes = sizes
        .iter()
        .enumerate()
        .map(|(index, &(w, h, channels))| {
            let row = w as usize * channels as usize * sample_bytes;
            let (data, stride) = (samples_frame.data(index), samples_frame.stride(index));
            let mut plane = Vec::with_capacity(row * h as usize);
            for y in 0..h as usize {
                plane.extend_from_slice(&data[y * stride..][..row]);
            }
            plane
        })
        .collect();
    let transfer = hdr::Transfer::of(tags);
    let colors = Colors::of_frame(tags);
    let full_range = colors.range == color::Range::JPEG
        || tags.format() == Pixel::YUVJ420P
        || samples_frame.format() == Pixel::YUVJ420P;
    let color = YuvColor {
        ycbcr_to_rgb: ycbcr_to_rgb(
            luma_weights(colors.space, tags.height()),
            full_range,
            format.depth,
            format.to_code,
        ),
        // As `hdr::ToneMapper` takes HDR to be.
        to_srgb_primaries: BT2020_TO_SRGB,
        transfer: match transfer {
            None => YuvTransfer::Sdr,
            Some(hdr::Transfer::Pq) => YuvTransfer::Pq,
            Some(hdr::Transfer::Hlg) => YuvTransfer::Hlg,
        },
        chroma_siting,
    };
    let frame = YuvFrame::new(width, height, format.layout, format.wide, planes, color)?;
    // As they were taken here, whatever size the planes are now.
    let range = if full_range {
        color::Range::JPEG
    } else {
        color::Range::MPEG
    };
    let space = match colors.space {
        color::Space::Unspecified if tags.height() >= 720 => color::Space::BT709,
        color::Space::Unspecified => color::Space::SMPTE170M,
        space => space,
    };
    Ok(YuvPicture {
        frame,
        hdr: transfer.is_some(),
        colors: Colors {
            range,
            space,
            ..colors
        },
    })
}

/// Where `frame`'s first chroma samples sit, in luma samples from its
/// top-left corner (`gpui::YuvColor::chroma_siting`). Video that doesn't say
/// is taken to have them in line with the first column, as MPEG-2 and the
/// codecs since do; JPEG has them between the first two.
fn chroma_siting(frame: &frame::Video) -> [f32; 2] {
    match frame.chroma_location() {
        chroma::Location::Left => [0.5, 1.],
        chroma::Location::Center => [1., 1.],
        chroma::Location::TopLeft => [0.5, 0.5],
        chroma::Location::Top => [1., 0.5],
        chroma::Location::BottomLeft => [0.5, 1.5],
        chroma::Location::Bottom => [1., 1.5],
        chroma::Location::Unspecified if frame.format() == Pixel::YUVJ420P => [1., 1.],
        chroma::Location::Unspecified => [0.5, 1.],
    }
}

/// The luma weights (Kr, Kb) a frame is encoded with, chosen as
/// `decoder::set_colorspace` chooses swscale's.
fn luma_weights(space: color::Space, height: u32) -> (f32, f32) {
    match space {
        color::Space::BT709 => (0.2126, 0.0722),
        color::Space::FCC => (0.30, 0.11),
        color::Space::SMPTE240M => (0.212, 0.087),
        color::Space::BT2020NCL | color::Space::BT2020CL => (0.2627, 0.0593),
        color::Space::Unspecified if height >= 720 => (0.2126, 0.0722),
        _ => (0.299, 0.114),
    }
}

/// Columns of the matrix from samples (y, cb, cr, 1), each read as
/// `code / to_code` of a `depth`-bit code, to nonlinear R'G'B' in 0..1.
fn ycbcr_to_rgb((kr, kb): (f32, f32), full_range: bool, depth: u32, to_code: f32) -> [[f32; 4]; 4] {
    let kg = 1. - kr - kb;
    let step = (1u32 << (depth - 8)) as f32;
    let (black, luma_range, center, chroma_range) = if full_range {
        let max = ((1u32 << depth) - 1) as f32;
        (0., max, 128. * step, max)
    } else {
        (16. * step, 219. * step, 128. * step, 224. * step)
    };
    let (y_scale, y_offset) = (to_code / luma_range, -black / luma_range);
    let (c_scale, c_offset) = (to_code / chroma_range, -center / chroma_range);
    let cb = [0., -2. * kb * (1. - kb) / kg, 2. * (1. - kb)];
    let cr = [2. * (1. - kr), -2. * kr * (1. - kr) / kg, 0.];
    let offset = |i: usize| y_offset + c_offset * (cb[i] + cr[i]);
    [
        [y_scale, y_scale, y_scale, 0.],
        [cb[0] * c_scale, cb[1] * c_scale, cb[2] * c_scale, 0.],
        [cr[0] * c_scale, cr[1] * c_scale, cr[2] * c_scale, 0.],
        [offset(0), offset(1), offset(2), 1.],
    ]
}

/// Linear BT.2020 RGB to linear sRGB (BT.709) RGB, as matrix columns.
const BT2020_TO_SRGB: [[f32; 4]; 4] = [
    [1.660_491, -0.124_55, -0.018_151, 0.],
    [-0.587_641, 1.1329, -0.100_579, 0.],
    [-0.072_85, -0.008_349, 1.118_73, 0.],
    [0., 0., 0., 1.],
];

#[cfg(test)]
#[path = "tests/yuv.rs"]
mod tests;
