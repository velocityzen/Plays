//! Frames turned and mirrored on the CPU, where VideoToolbox can't turn
//! them (before macOS 13): once they are scaled to the window, so there are
//! few samples to move. Each plane turns alike, a sample at a time: a luma
//! sample, a pair of chroma samples, or a BGRA pixel.

use rayon::prelude::*;

use crate::media::Orientation;

/// Lay a plane of `size` samples, across and down, into `turned` as
/// `orientation` turns it: mirrored, then turned clockwise. Rows of each
/// are a stride of samples apart; `turned` is as big as the plane turned.
pub fn turn<T: Copy + Send + Sync>(
    (plane, stride): (&[T], usize),
    (width, height): (usize, usize),
    (turned, turned_stride): (&mut [T], usize),
    orientation: Orientation,
) {
    let (across, down) = if orientation.sideways() {
        (height, width)
    } else {
        (width, height)
    };
    // Where a turned sample comes from: turned back, then mirrored back.
    let from = |x: isize, y: isize| {
        let (mut x, mut y, mut wide) = (x, y, across as isize);
        let mut tall = down as isize;
        for _ in 0..orientation.clockwise_turns() {
            (x, y) = (y, wide - 1 - x);
            (wide, tall) = (tall, wide);
        }
        if orientation.is_mirrored() {
            x = wide - 1 - x;
        }
        (x, y)
    };
    // It's the same step from each sample to the next, along and down.
    let start = from(0, 0);
    let step = |(x, y): (isize, isize)| (x - start.0, y - start.1);
    let (along, next_row) = (step(from(1, 0)), step(from(0, 1)));
    turned
        .par_chunks_mut(turned_stride.max(1))
        .take(down)
        .enumerate()
        .for_each(|(y, row)| {
            let y = y as isize;
            let (mut x0, mut y0) = (start.0 + next_row.0 * y, start.1 + next_row.1 * y);
            for sample in row.iter_mut().take(across) {
                *sample = plane[y0 as usize * stride + x0 as usize];
                x0 += along.0;
                y0 += along.1;
            }
        });
}

#[cfg(test)]
#[path = "tests/turn.rs"]
mod tests;
