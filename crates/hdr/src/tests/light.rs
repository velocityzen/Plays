use super::*;

fn close(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn pq_matches_known_levels() {
    assert_eq!(pq_to_nits(0.0), 0.0);
    assert!(close(pq_to_nits(1.0), 10000.0, 1.0));
    // Published PQ code values for 100 and 203 nits.
    assert!(close(pq_to_nits(0.5081), 100.0, 0.5));
    assert!(close(pq_to_nits(0.5806), 203.0, 1.0));
}

#[test]
fn pq_codes_no_light_past_its_ends() {
    assert!(close(nits_to_pq(PQ_PEAK_NITS), 1.0, 1e-6));
    assert_eq!(nits_to_pq(20_000.0), nits_to_pq(PQ_PEAK_NITS));
    assert_eq!(nits_to_pq(-5.0), nits_to_pq(0.0));
    assert_eq!(pq_to_nits(2.0), pq_to_nits(1.0));
    assert_eq!(pq_to_nits(-1.0), 0.0);
}

/// sRGB's curve at points worked out apart from this code, from IEC
/// 61966-2-1, either side of its knee.
#[test]
fn srgb_matches_known_levels() {
    assert!(close(srgb_encode(0.01), 0.099_853, 1e-5));
    assert!(close(srgb_encode(0.18), 0.461_358, 1e-5));
    assert!(close(srgb_encode(0.002), 0.025_84, 1e-6));
    assert!(close(srgb_decode(0.5), 0.214_041, 1e-5));
    assert!(close(srgb_decode(0.03), 0.002_322, 1e-6));
    assert!(close(srgb_decode(0.07), 0.005_981, 1e-6));
}

#[test]
fn hlg_reference_white_is_203_nits() {
    assert!(close(hlg_to_scene(0.5), 1.0 / 12.0, 1e-6));
    assert!(close(hlg_to_scene(1.0), 1.0, 1e-3));
    // BT.2408: 75% HLG grey is reference white on a 1000-nit display.
    let grey = hlg_to_scene(0.75);
    let nits = hlg_scene_to_nits([grey; 3]);
    assert!(close(nits[1], REFERENCE_WHITE_NITS, 3.0), "{nits:?}");
}

#[test]
fn white_stays_white_across_primaries() {
    for c in bt2020_to_bt709([1.0; 3])
        .into_iter()
        .chain(bt709_to_bt2020([1.0; 3]))
    {
        assert!(close(c, 1.0, 1e-3));
    }
    let color = [0.8, 0.3, 0.1];
    for (a, b) in bt2020_to_bt709(bt709_to_bt2020(color))
        .into_iter()
        .zip(color)
    {
        assert!(close(a, b, 1e-4), "{a} {b}");
    }
}

/// PQ's code for `nits`, in doubles.
fn pq(nits: f64) -> f64 {
    let (m1, m2) = (f64::from(PQ_M1), f64::from(PQ_M2));
    let (c1, c2, c3) = (f64::from(PQ_C1), f64::from(PQ_C2), f64::from(PQ_C3));
    let y = (nits / 10000.0).powf(m1);
    ((c1 + c2 * y) / (1.0 + c3 * y)).powf(m2)
}

#[test]
fn encodes_light_back_into_signals() {
    for signal in [0.0, 0.1, 0.5081, 0.5806, 0.9, 1.0] {
        assert!(
            close(nits_to_pq(pq_to_nits(signal)), signal, 1e-4),
            "{signal}"
        );
        assert!(
            close(scene_to_hlg(hlg_to_scene(signal)), signal, 1e-4),
            "{signal}"
        );
    }
    for nits in [0.005, 0.05, 0.5, 5.0, 50.0, 203.0, 1000.0, 4000.0, 10000.0] {
        assert!(
            (f64::from(nits_to_pq(nits)) - pq(f64::from(nits))).abs() < 1e-5,
            "{nits}"
        );
    }
    for srgb in [0.0, 0.02, 0.5, 1.0] {
        assert!(close(srgb_encode(srgb_decode(srgb)), srgb, 1e-5), "{srgb}");
    }
    let scene = [0.3, 0.2, 0.1];
    let back = hlg_nits_to_scene(hlg_scene_to_nits(scene));
    for (a, b) in back.into_iter().zip(scene) {
        assert!(close(a, b, 1e-5), "{a} {b}");
    }
    assert_eq!(hlg_nits_to_scene([0.0; 3]), [0.0; 3]);
    // SDR white lands on HDR reference white.
    let white = sdr_to_hdr([1.0; 3], Transfer::Pq);
    assert!(white.iter().all(|&c| close(c, 0.5806, 1e-3)), "{white:?}");
    let white = sdr_to_hdr([1.0; 3], Transfer::Hlg);
    assert!(white.iter().all(|&c| close(c, 0.75, 5e-3)), "{white:?}");
    assert_eq!(sdr_to_hdr([0.0; 3], Transfer::Hlg), [0.0; 3]);
}
