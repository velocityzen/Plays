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
