//! How a tile shows its video: turned and mirrored as the file says and
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

/// What the viewer changed about a tile's picture.
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
mod tests {
    use gpui::point;

    use super::*;

    const HD: (u32, u32) = (1920, 1080);

    fn framing(orientation: Orientation, aspect: Option<Ratio>, crop: Option<Ratio>) -> Framing {
        let adjustments = Adjustments {
            orientation,
            aspect,
            crop,
        };
        Framing::new(HD, Orientation::UPRIGHT, &adjustments).unwrap()
    }

    fn close(a: ContentTransform, b: ContentTransform) -> bool {
        let all = |t: ContentTransform| [t.origin, t.across, t.down].concat();
        all(a).iter().zip(all(b)).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    fn sample(t: ContentTransform, (u, v): (f32, f32)) -> (f32, f32) {
        (
            t.origin[0] + u * t.across[0] + v * t.down[0],
            t.origin[1] + u * t.across[1] + v * t.down[1],
        )
    }

    #[test]
    fn shows_the_picture_as_it_is_by_default() {
        let plain = framing(Orientation::UPRIGHT, None, None);
        assert_eq!(plain.size, (1920.0, 1080.0));
        assert!(close(plain.content(), ContentTransform::IDENTITY));
        assert_eq!(
            Framing::new((0, 10), Orientation::UPRIGHT, &Adjustments::default()),
            None
        );
    }

    #[test]
    fn turns_and_mirrors() {
        // A quarter turn clockwise: the top left corner shows the bottom
        // left of the picture, and going right goes up it.
        let turned = framing(Orientation::new(1, false), None, None);
        assert_eq!(turned.size, (1080.0, 1920.0));
        let expected = ContentTransform {
            origin: [0.0, 1.0],
            across: [0.0, -1.0],
            down: [1.0, 0.0],
        };
        assert!(close(turned.content(), expected));
        let mirrored = framing(Orientation::UPRIGHT.flipped_horizontally(), None, None);
        let expected = ContentTransform {
            origin: [1.0, 0.0],
            across: [-1.0, 0.0],
            down: [0.0, 1.0],
        };
        assert!(close(mirrored.content(), expected));
        // The viewer's turn comes after the file's.
        let phone = Framing::new(
            HD,
            Orientation::new(1, false),
            &Adjustments {
                orientation: Orientation::new(1, false),
                ..Adjustments::default()
            },
        )
        .unwrap();
        assert_eq!(phone.size, (1920.0, 1080.0));
        assert!(close(
            phone.content(),
            framing(Orientation::new(2, false), None, None).content()
        ));
    }

    #[test]
    fn changes_the_aspect_ratio_before_turning() {
        let square = Some(ASPECT_RATIOS[0]);
        assert_eq!(
            framing(Orientation::UPRIGHT, square, None).size,
            (1080.0, 1080.0)
        );
        let wide = Some(ASPECT_RATIOS[5]);
        let turned = framing(Orientation::new(1, false), wide, None);
        assert!((turned.size.0 - 1080.0).abs() < 0.01);
        assert!((turned.size.1 - 1080.0 * 2.35).abs() < 0.01);
        // Squeezing samples all of the picture still.
        assert!(close(
            turned.content(),
            framing(Orientation::new(1, false), None, None).content()
        ));
    }

    #[test]
    fn crops_the_middle_as_it_shows() {
        let square = framing(Orientation::UPRIGHT, None, Some(CROPS[0]));
        assert_eq!(square.size, (1080.0, 1080.0));
        let expected = ContentTransform {
            origin: [0.21875, 0.0],
            across: [0.5625, 0.0],
            down: [0.0, 1.0],
        };
        assert!(close(square.content(), expected));
        // Wider than the picture: the top and bottom go.
        let scope = framing(Orientation::UPRIGHT, None, Some(CROPS[4]));
        assert_eq!(scope.size.0, 1920.0);
        assert!((scope.size.1 - 1920.0 / 2.39).abs() < 0.01);
        // Turned on its side first: 1080 wide, so 16:9 keeps 1080x607.5 of
        // the 1920 tall picture, which is across the decoded one.
        let turned = framing(Orientation::new(1, false), None, Some(CROPS[2]));
        assert!((turned.size.0 - 1080.0).abs() < 0.01);
        assert!((turned.size.1 - 607.5).abs() < 0.01);
    }

    /// A bitmap placed on the shown picture lands where the picture shows
    /// the part of it the bitmap covers.
    #[test]
    fn places_bitmaps_where_the_picture_shows_them() {
        let rect = [0.1, 0.7, 0.5, 0.2];
        for i in 0..8 {
            let orientation = Orientation::new(i % 4, i >= 4);
            for crop in [None, Some(CROPS[0]), Some(CROPS[4])] {
                let framing = framing(orientation, None, crop);
                let ([x, y, w, h], bitmap) = framing.place(rect);
                // The corners of where it's placed sample the corners of the
                // rect, and the bitmap's own sampling turns the same way.
                let picture = framing.content();
                for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                    let (px, py) = sample(picture, (x + u * w, y + v * h));
                    let (bx, by) = sample(bitmap, (u, v));
                    assert!((px - (rect[0] + bx * rect[2])).abs() < 1e-5, "{i} {crop:?}");
                    assert!((py - (rect[1] + by * rect[3])).abs() < 1e-5, "{i} {crop:?}");
                }
            }
        }
    }

    #[test]
    fn fits_or_covers() {
        let tile = Bounds {
            origin: point(px(10.), px(20.)),
            size: size(px(400.), px(400.)),
        };
        let contain = fit(tile, (1920.0, 1080.0), false);
        assert_eq!(contain.size, size(px(400.), px(225.)));
        assert_eq!(contain.origin, point(px(10.), px(107.5)));
        let cover = fit(tile, (1920.0, 1080.0), true);
        assert_eq!(cover.size.height, px(400.));
        assert!((f32::from(cover.size.width) - 711.11).abs() < 0.01);
        assert_eq!(fit(tile, (0.0, 0.0), false), tile);
    }

    #[test]
    fn cycles_through_ratios() {
        assert_eq!(next_ratio(None, &CROPS), Some(CROPS[0]));
        assert_eq!(next_ratio(Some(CROPS[0]), &CROPS), Some(CROPS[1]));
        assert_eq!(next_ratio(Some(CROPS[4]), &CROPS), None);
    }
}
