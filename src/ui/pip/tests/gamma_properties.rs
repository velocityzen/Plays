use hegel::TestCase;
use hegel::generators as gs;

use super::*;
use crate::settings::GAMMA_RANGE;

/// A gamma the settings offer, and a kind of frame.
fn curve(tc: &TestCase) -> Curve {
    let gamma = tc.draw(
        gs::floats::<f32>()
            .min_value(*GAMMA_RANGE.start())
            .max_value(*GAMMA_RANGE.end()),
    );
    let transfer = tc.draw(gs::sampled_from(vec![
        None,
        Some(Transfer::Pq),
        Some(Transfer::Hlg),
    ]));
    Curve::new(gamma, transfer)
}

#[hegel::test]
fn any_curve_keeps_signals_in_order_and_in_range(tc: TestCase) {
    let curve = curve(&tc);
    let mut signals = tc.draw(gs::vecs(gs::floats::<f32>().min_value(-0.5).max_value(1.5)));
    signals.sort_by(f32::total_cmp);
    let bent: Vec<f32> = signals.iter().map(|&signal| curve.at(signal)).collect();
    assert!(bent.is_sorted(), "{signals:?} became {bent:?}");
    assert!(bent.iter().all(|signal| (0.0..=1.0).contains(signal)));
}

#[hegel::test]
fn any_frame_is_bent_within_its_rows(tc: TestCase) {
    let curve = curve(&tc);
    let width = tc.draw(gs::integers::<usize>().min_value(1).max_value(9));
    let height = tc.draw(gs::integers::<usize>().min_value(1).max_value(9));
    let (luma_stride, chroma_stride) = (
        width + tc.draw(gs::integers::<usize>().max_value(3)),
        width.div_ceil(2) * 2 + tc.draw(gs::integers::<usize>().max_value(3)),
    );
    let past = 0xab;
    let samples = |stride: usize, rows: usize, wide: usize| {
        let row = gs::vecs(gs::integers::<u8>()).min_size(wide).max_size(wide);
        let rows = gs::vecs(row).min_size(rows).max_size(rows);
        tc.draw(rows)
            .into_iter()
            .flat_map(|mut row| {
                row.resize(stride, past);
                row
            })
            .collect::<Vec<u8>>()
    };
    let mut luma = samples(luma_stride, height, width);
    let mut chroma = samples(chroma_stride, height.div_ceil(2), width.div_ceil(2) * 2);
    let ycc = Ycc {
        kr: 0.2126,
        kb: 0.0722,
        bits: 8,
        full_range: tc.draw(gs::booleans()),
        shift: 0,
    };
    curve.bend_ycbcr(
        (&mut luma, luma_stride),
        (&mut chroma, chroma_stride),
        width,
        ycc,
    );
    let untouched = |plane: &[u8], stride: usize, wide: usize| {
        plane
            .chunks(stride)
            .all(|row| row[wide..].iter().all(|&sample| sample == past))
    };
    assert!(untouched(&luma, luma_stride, width));
    assert!(untouched(&chroma, chroma_stride, width.div_ceil(2) * 2));
}
