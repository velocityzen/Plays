use super::*;
use crate::REFERENCE_WHITE_NITS;

#[test]
fn reads_pq_between_its_steps_as_its_curve_goes() {
    let tables = Tables::get();
    for step in 0..=10_000 {
        let signal = step as f32 / 10_000.0;
        let nits = pq_to_nits(signal);
        assert!(
            (tables.pq_nits(signal) - nits).abs() <= 1e-4 * nits.max(1.0),
            "{signal}"
        );
        // Far within a 10-bit code, from black to PQ's peak.
        assert!(
            (tables.pq_signal(nits) - signal).abs() < 1e-4,
            "{signal}: {nits} nits"
        );
    }
    assert_eq!(tables.pq_nits(-1.0), 0.0, "no signal under none");
    assert!(
        (tables.pq_nits(2.0) - PQ_PEAK_NITS).abs() < 0.5,
        "none over 1"
    );
    assert_eq!(
        tables.pq_signal(-1.0),
        tables.pq_signal(0.0),
        "no light under none"
    );
    assert!(
        (tables.pq_signal(20_000.0) - 1.0).abs() < 1e-6,
        "none over its peak"
    );
    // Reference white, between steps.
    assert!((tables.pq_nits(0.5806) - REFERENCE_WHITE_NITS).abs() < 1.0);
}

/// A mastering display's black and the darkest steps of 12 bits of PQ are
/// told apart: the steps crowd there.
#[test]
fn encodes_pq_finely_through_the_dark() {
    let tables = Tables::get();
    for nits in [0.0001, 0.005, 0.05, 0.5, 5.0] {
        let exact = nits_to_pq(nits);
        assert!(
            (tables.pq_signal(nits) - exact).abs() < 1.0 / 65536.0,
            "{nits}: {} {exact}",
            tables.pq_signal(nits)
        );
    }
}

#[test]
fn reads_hlg_between_its_steps_as_its_curve_goes() {
    let tables = Tables::get();
    for step in 0..=10_000 {
        let signal = step as f32 / 10_000.0;
        let scene = hlg_to_scene(signal);
        assert!(
            (tables.hlg_scene(signal) - scene).abs() <= 1e-4 * scene.max(1e-3),
            "{signal}"
        );
    }
    assert_eq!(tables.hlg_scene(-1.0), 0.0);
    assert!((tables.hlg_scene(2.0) - 1.0).abs() < 1e-3);
}

#[test]
fn the_tables_are_made_once() {
    assert!(std::ptr::eq(Tables::get(), Tables::get()));
}
