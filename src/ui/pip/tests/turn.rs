use super::*;
use crate::ui::framing::{Adjustments, Framing};

/// The eight ways a picture goes.
fn every_way() -> impl Iterator<Item = Orientation> {
    (0..4).flat_map(|turns| [false, true].map(|mirrored| Orientation::new(turns, mirrored)))
}

#[test]
fn every_way_puts_each_sample_where_the_block_shows_it() {
    // 3 × 2, each sample its own.
    let (width, height) = (3, 2);
    let plane: Vec<u16> = (0..6).collect();
    for orientation in every_way() {
        let (across, down) = if orientation.sideways() {
            (2, 3)
        } else {
            (3, 2)
        };
        let mut turned = vec![u16::MAX; 6];
        turn(
            (&plane, width),
            (width, height),
            (&mut turned, across),
            orientation,
        );
        let adjustments = Adjustments {
            orientation,
            ..Adjustments::default()
        };
        let framing = Framing::new((3, 2), Orientation::UPRIGHT, &adjustments).unwrap();
        for y in 0..height {
            for x in 0..width {
                let center = [(x as f32 + 0.5) / 3.0, (y as f32 + 0.5) / 2.0, 0.0, 0.0];
                let [shown_x, shown_y, ..] = framing.place(center).0;
                let at =
                    (shown_y * down as f32) as usize * across + (shown_x * across as f32) as usize;
                assert_eq!(
                    turned[at],
                    plane[y * width + x],
                    "{orientation:?} at {x}, {y}"
                );
            }
        }
    }
}

#[test]
fn only_the_samples_are_turned_and_only_into_the_turned_samples() {
    // 2 × 3 in rows 4 apart, into rows 5 apart, a row more than it needs.
    let past = 99u8;
    let plane = [1, 2, past, past, 3, 4, past, past, 5, 6, past, past];
    for orientation in every_way() {
        let (across, down) = if orientation.sideways() {
            (3, 2)
        } else {
            (2, 3)
        };
        let mut turned = [past; 20];
        turn((&plane, 4), (2, 3), (&mut turned, 5), orientation);
        let mut samples = Vec::new();
        for (y, row) in turned.chunks(5).enumerate() {
            for (x, &sample) in row.iter().enumerate() {
                if x < across && y < down {
                    samples.push(sample);
                } else {
                    assert_eq!(sample, past, "{orientation:?} at {x}, {y}");
                }
            }
        }
        samples.sort_unstable();
        assert_eq!(samples, [1, 2, 3, 4, 5, 6], "{orientation:?}");
    }
}

#[test]
fn turning_one_way_then_another_is_turning_both_at_once() {
    let plane: Vec<u32> = (0..12).collect();
    let turned = |plane: &[u32], (width, height): (usize, usize), orientation: Orientation| {
        let across = if orientation.sideways() {
            height
        } else {
            width
        };
        let mut turned = vec![0; 12];
        turn(
            (plane, width),
            (width, height),
            (&mut turned, across),
            orientation,
        );
        turned
    };
    for first in every_way() {
        let between = if first.sideways() { (3, 4) } else { (4, 3) };
        let once = turned(&plane, (4, 3), first);
        for then in every_way() {
            let twice = turned(&once, between, then);
            assert_eq!(
                twice,
                turned(&plane, (4, 3), then.after(first)),
                "{first:?}, {then:?}"
            );
        }
    }
}

#[test]
fn nothing_to_turn_is_left_as_it_is() {
    let mut turned: [u8; 0] = [];
    turn(
        (&[], 0),
        (0, 3),
        (&mut turned, 0),
        Orientation::new(1, true),
    );
}
