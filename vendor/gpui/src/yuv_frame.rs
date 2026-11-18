//! Plays: video frames as their decoder left them, planes of Y'CbCr
//! samples, which the Linux renderer uploads to textures of their own and
//! turns into RGB as it draws them (`Window::paint_yuv_frame`), rather than
//! the CPU converting them and the sprite atlas taking them in.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Result, ensure};

/// How a frame's chroma planes are laid out. Both are 4:2:0: a chroma
/// sample for every 2x2 luma samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum YuvLayout {
    /// Y, then Cb, then Cr, each a plane of its own (FFmpeg's `yuv420p`).
    Planar,
    /// Y, then Cb and Cr interleaved in one plane (`nv12`, `p010`).
    SemiPlanar,
}

/// How a frame's samples encode light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum YuvTransfer {
    /// SDR: R'G'B' is shown as it is.
    Sdr,
    /// HDR10's SMPTE ST 2084.
    Pq,
    /// Hybrid log-gamma.
    Hlg,
}

/// How a frame's samples become colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct YuvColor {
    /// Columns of the matrix from a pixel's samples (y, cb, cr, 1), each
    /// read as a fraction of the largest its size holds (an 8-bit sample's
    /// code over 255, a 16-bit one's over 65535), to nonlinear R'G'B' in 0
    /// to 1. It carries the matrix coefficients, the range and where in its
    /// 16 bits a 10-bit sample is.
    pub ycbcr_to_rgb: [[f32; 4]; 4],
    /// Columns of the matrix from the video's linear RGB to sRGB's, for HDR.
    pub to_srgb_primaries: [[f32; 4]; 4],
    /// How R'G'B' encodes light.
    pub transfer: YuvTransfer,
    /// Where the first Cb and Cr samples sit, in luma samples from the
    /// frame's top-left corner (the next are 2 further on): `[1., 1.]`
    /// between the first two columns and rows, as in JPEG; `[0.5, 1.]` in
    /// line with the first column, as in most video.
    pub chroma_siting: [f32; 2],
}

/// A frame's planes: its samples and how to show them. Cheap to clone.
#[derive(Clone, Debug)]
pub struct YuvFrame(Arc<YuvFrameData>);

#[derive(Debug)]
struct YuvFrameData {
    id: u64,
    width: u32,
    height: u32,
    layout: YuvLayout,
    wide: bool,
    planes: Vec<Vec<u8>>,
    color: YuvColor,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl YuvFrame {
    /// A frame `width` x `height` luma samples, `wide` if its samples are
    /// 16 bits (little-endian) rather than 8. `planes` are as `layout`
    /// says, each row after row with nothing between them.
    pub fn new(
        width: u32,
        height: u32,
        layout: YuvLayout,
        wide: bool,
        planes: Vec<Vec<u8>>,
        color: YuvColor,
    ) -> Result<Self> {
        ensure!(width > 0 && height > 0, "a frame has no samples");
        let frame = YuvFrameData {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            layout,
            wide,
            planes,
            color,
        };
        let expected = frame.plane_sizes();
        ensure!(
            frame.planes.len() == expected.len(),
            "{:?} has {} planes, not {}",
            layout,
            expected.len(),
            frame.planes.len()
        );
        for (index, (plane, (w, h, channels))) in frame.planes.iter().zip(&expected).enumerate() {
            let bytes = *w as usize * *h as usize * *channels as usize * frame.sample_bytes();
            ensure!(
                plane.len() == bytes,
                "plane {index} has {} bytes, not {bytes}",
                plane.len()
            );
        }
        Ok(Self(Arc::new(frame)))
    }

    /// Tells frames apart, for the renderer to upload each once.
    pub fn id(&self) -> u64 {
        self.0.id
    }

    /// Its size in luma samples.
    pub fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }

    /// How its chroma planes are laid out.
    pub fn layout(&self) -> YuvLayout {
        self.0.layout
    }

    /// Whether its samples are 16 bits.
    pub fn is_wide(&self) -> bool {
        self.0.wide
    }

    /// How its samples become colors.
    pub fn color(&self) -> &YuvColor {
        &self.0.color
    }

    /// Each plane's samples.
    pub fn planes(&self) -> &[Vec<u8>] {
        &self.0.planes
    }

    /// Each plane's width and height in samples, and how many samples each
    /// position holds (2 for interleaved Cb and Cr).
    pub fn plane_sizes(&self) -> Vec<(u32, u32, u32)> {
        self.0.plane_sizes()
    }
}

impl YuvFrameData {
    fn sample_bytes(&self) -> usize {
        if self.wide { 2 } else { 1 }
    }

    fn plane_sizes(&self) -> Vec<(u32, u32, u32)> {
        let luma = (self.width, self.height, 1);
        let (cw, ch) = (self.width.div_ceil(2), self.height.div_ceil(2));
        match self.layout {
            YuvLayout::Planar => vec![luma, (cw, ch, 1), (cw, ch, 1)],
            YuvLayout::SemiPlanar => vec![luma, (cw, ch, 2)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLOR: YuvColor = YuvColor {
        ycbcr_to_rgb: [[0.; 4]; 4],
        to_srgb_primaries: [[0.; 4]; 4],
        transfer: YuvTransfer::Sdr,
        chroma_siting: [1., 1.],
    };

    #[test]
    fn checks_its_planes() {
        let planes = vec![vec![0; 5 * 3], vec![0; 3 * 2 * 2]];
        let frame = YuvFrame::new(5, 3, YuvLayout::SemiPlanar, false, planes, COLOR).unwrap();
        assert_eq!(frame.plane_sizes(), [(5, 3, 1), (3, 2, 2)]);
        let wide = vec![vec![0; 4 * 2 * 2], vec![0; 2 * 2], vec![0; 2 * 2]];
        assert!(YuvFrame::new(4, 2, YuvLayout::Planar, true, wide, COLOR).is_ok());
        let short = vec![vec![0; 4 * 2], vec![0; 2]];
        assert!(YuvFrame::new(4, 2, YuvLayout::SemiPlanar, false, short, COLOR).is_err());
        let other = YuvFrame::new(1, 1, YuvLayout::Planar, false, vec![vec![0]; 3], COLOR).unwrap();
        assert_ne!(frame.id(), other.id());
    }
}
