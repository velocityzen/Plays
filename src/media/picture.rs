//! Decoded pictures in the forms gpui can draw.

use std::sync::Arc;

use gpui::{ImageId, RenderImage};

#[cfg(target_os = "macos")]
pub use super::videotoolbox::Surface;

/// A decoded picture, ready to draw.
#[derive(Clone)]
pub enum Picture {
    /// BGRA pixels in memory; gpui uploads them to its texture atlas when
    /// drawn, and they must be dropped from it once off screen.
    Image(Arc<RenderImage>),
    /// A hardware-decoded frame that stays in GPU memory and is drawn as is.
    #[cfg(target_os = "macos")]
    Surface(Surface),
}

/// Identifies a picture, to tell whether it is the one that was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureId {
    Image(ImageId),
    #[cfg(target_os = "macos")]
    Surface(u64),
}

impl Picture {
    pub fn id(&self) -> PictureId {
        match self {
            Picture::Image(image) => PictureId::Image(image.id),
            #[cfg(target_os = "macos")]
            Picture::Surface(surface) => PictureId::Surface(surface.id()),
        }
    }
}
