//! How a block shows its video: turned and mirrored as the file says and
//! then as the viewer asks, at another aspect ratio if asked, and cropped to
//! another shape. All of it happens as the picture is drawn (gpui samples
//! its texture turned and cropped), so a change shows at once, paused or
//! not, and costs nothing per frame.

use gpui::{Bounds, ContentTransform, Pixels, px, size};

use crate::media::Orientation;

/// A shape, width over height, as the menus name it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ratio {
    pub name: &'static str,
    pub value: f32,
}

const fn ratio(name: &'static str, value: f32) -> Ratio {
    Ratio { name, value }
}

/// The aspect ratios a picture can be shown at instead of its own.
pub const ASPECT_RATIOS: [Ratio; 6] = [
    ratio("1:1", 1.0),
    ratio("4:3", 4.0 / 3.0),
    ratio("16:10", 16.0 / 10.0),
    ratio("16:9", 16.0 / 9.0),
    ratio("1.85:1", 1.85),
    ratio("2.35:1", 2.35),
];

/// The shapes a picture can be cropped to.
pub const CROPS: [Ratio; 5] = [
    ratio("1:1", 1.0),
    ratio("4:3", 4.0 / 3.0),
    ratio("16:9", 16.0 / 9.0),
    ratio("1.85:1", 1.85),
    ratio("2.39:1", 2.39),
];

/// The one after `current` in `ratios`, then none (as the file is), then
/// the first again.
pub fn next_ratio(current: Option<Ratio>, ratios: &[Ratio]) -> Option<Ratio> {
    match current.and_then(|c| ratios.iter().position(|r| *r == c)) {
        Some(i) => ratios.get(i + 1).copied(),
        None => ratios.first().copied(),
    }
}

/// What the viewer changed about a block's picture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Adjustments {
    /// Turned and mirrored, after the file's own orientation.
    pub orientation: Orientation,
    /// The width over height to show the picture at, as decoded (before
    /// it's turned), instead of its own.
    pub aspect: Option<Ratio>,
    /// The shape to crop the picture to as it shows (turned), centered.
    pub crop: Option<Ratio>,
}

/// How a picture of some size shows, with its adjustments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    /// Width and height shown, in the video's display pixels.
    pub size: (f32, f32),
    /// The file's orientation, then the viewer's.
    orientation: Orientation,
    /// The part of the turned picture kept, across and down (centered).
    kept: (f32, f32),
}

impl Framing {
    /// For a video of display size `video` (as decoded, after its pixel
    /// aspect ratio) that the file says to turn to `stream`. `None` without
    /// a size.
    pub fn new(video: (u32, u32), stream: Orientation, adjustments: &Adjustments) -> Option<Self> {
        let (width, height) = (video.0 as f32, video.1 as f32);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let width = adjustments
            .aspect
            .map_or(width, |aspect| height * aspect.value);
        let orientation = adjustments.orientation.after(stream);
        let (width, height) = if orientation.sideways() {
            (height, width)
        } else {
            (width, height)
        };
        let kept = match adjustments.crop {
            Some(crop) if width / height > crop.value => (height * crop.value / width, 1.0),
            Some(crop) => (1.0, width / crop.value / height),
            None => (1.0, 1.0),
        };
        Some(Self {
            size: (width * kept.0, height * kept.1),
            orientation,
            kept,
        })
    }

    /// Where the picture goes in `bounds`: fitted in them, or covering them.
    pub fn fit(&self, bounds: Bounds<Pixels>, cover: bool) -> Bounds<Pixels> {
        fit(bounds, self.size, cover)
    }

    /// How the decoded picture is turned: the file's way, then the viewer's.
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// How to sample the decoded picture to draw it in its place.
    pub fn content(&self) -> ContentTransform {
        sampling(self.orientation, self.kept)
    }

    /// Where a part of the decoded picture (`[x, y, width, height]`, in
    /// fractions of it, as bitmap subtitles are placed) shows, in fractions
    /// of the shown picture (past 0 to 1 where cropped away), and how to
    /// sample a picture of just that part to draw it there.
    pub fn place(&self, rect: [f32; 4]) -> ([f32; 4], ContentTransform) {
        let [x, y, width, height] = rect;
        let m = self.orientation.matrix();
        let (fx, fy) = self.kept;
        // Turned about the picture's center, then into the part kept.
        let shown = |(sx, sy): (f32, f32)| {
            let (cx, cy) = (sx - 0.5, sy - 0.5);
            let (tx, ty) = (m[0][0] * cx + m[0][1] * cy, m[1][0] * cx + m[1][1] * cy);
            (tx / fx + 0.5, ty / fy + 0.5)
        };
        let (ax, ay) = shown((x, y));
        let (bx, by) = shown((x + width, y + height));
        let (left, top) = (ax.min(bx), ay.min(by));
        let placed = [left, top, ax.max(bx) - left, ay.max(by) - top];
        (placed, sampling(self.orientation, (1.0, 1.0)))
    }
}

/// Sampling for a picture turned to `orientation`, of which the part
/// `kept` (across and down, centered) shows. A point of the shown picture,
/// in fractions of it, is one of the turned picture, which turns back into
/// the decoded one by the transpose of the orientation's matrix.
fn sampling(orientation: Orientation, (fx, fy): (f32, f32)) -> ContentTransform {
    let m = orientation.matrix();
    let (cx, cy) = (-fx / 2.0, -fy / 2.0);
    ContentTransform {
        origin: [
            0.5 + m[0][0] * cx + m[1][0] * cy,
            0.5 + m[0][1] * cx + m[1][1] * cy,
        ],
        across: [fx * m[0][0], fx * m[0][1]],
        down: [fy * m[1][0], fy * m[1][1]],
    }
}

/// A picture of `picture` size fitted in `bounds` (letterboxed) or
/// covering them (overflowing), centered.
pub fn fit(bounds: Bounds<Pixels>, (width, height): (f32, f32), cover: bool) -> Bounds<Pixels> {
    if width <= 0.0 || height <= 0.0 {
        return bounds;
    }
    let scale_x = f32::from(bounds.size.width) / width;
    let scale_y = f32::from(bounds.size.height) / height;
    let scale = if cover {
        scale_x.max(scale_y)
    } else {
        scale_x.min(scale_y)
    };
    Bounds::centered_at(bounds.center(), size(px(width * scale), px(height * scale)))
}

#[cfg(test)]
#[path = "tests/framing.rs"]
mod tests;
