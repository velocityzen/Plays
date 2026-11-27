use std::collections::HashSet;
use std::sync::Mutex;

use super::*;

#[test]
fn rows_are_made_by_threads_kept_for_them() {
    let seen = Mutex::new(HashSet::new());
    for _ in 0..20 {
        let made = rows(4, 1024, |y, out: &mut [u8]| {
            seen.lock().unwrap().insert(thread::current().id());
            out.fill(y as u8);
        });
        assert!(
            made.chunks(4)
                .enumerate()
                .all(|(y, row)| row == [y as u8; 4])
        );
    }
    let threads = seen.into_inner().unwrap().len();
    let kept = pool().map_or(0, rayon::ThreadPool::current_num_threads);
    assert!(threads <= kept + 1, "{threads} threads for 20 frames");
}

#[test]
fn runs_over_a_last_row_shorter_than_the_rest() {
    // 200 rows of 3, in bands, the last only 1 long, as a frame's plane's
    // last row is.
    let mut pixels = vec![0u8; 3 * 199 + 1];
    let seen = Mutex::new(Vec::new());
    in_rows(&mut pixels, 3, |y, row| {
        seen.lock().unwrap().push((y, row.len()));
        row.fill(y as u8);
    });
    let mut seen = seen.into_inner().unwrap();
    seen.sort_unstable();
    let expected: Vec<_> = (0..200)
        .map(|y| (y, if y == 199 { 1 } else { 3 }))
        .collect();
    assert_eq!(seen, expected);
    assert!(
        pixels
            .chunks(3)
            .enumerate()
            .all(|(y, row)| row.iter().all(|&pixel| pixel == y as u8))
    );
}

#[test]
fn gamma_keeps_black_and_white() {
    for gamma in [0.5, 1.0, 1.4, 2.0] {
        let table = gamma_table(gamma);
        assert_eq!((table[0], table[255]), (0, 255));
    }
    assert_eq!(gamma_table(1.0), std::array::from_fn(|i| i as u8));
    // Above 1 lifts the middle, below 1 sinks it.
    assert!(gamma_table(1.5)[128] > 150);
    assert!(gamma_table(0.67)[128] < 100);
    // HDR highlights bend the other way round, toward white.
    assert!(apply_gamma(2.0, 1.5) < 2.0 && apply_gamma(2.0, 0.67) > 2.0);
    assert_eq!(apply_gamma(-0.25, 2.0), -0.5);
}
