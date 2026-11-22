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
    for srgb in [0.0, 0.02, 0.5, 1.0] {
        assert!(close(srgb_encode(srgb_decode(srgb)), srgb, 1e-5), "{srgb}");
    }
    let scene = [0.3, 0.2, 0.1];
    let back = hlg_nits_to_scene(hlg_scene_to_nits(scene));
    for (a, b) in back.into_iter().zip(scene) {
        assert!(close(a, b, 1e-5), "{a} {b}");
    }
    // SDR white lands on HDR reference white.
    let white = sdr_to_hdr([1.0; 3], Transfer::Pq);
    assert!(white.iter().all(|&c| close(c, 0.5806, 1e-3)), "{white:?}");
    let white = sdr_to_hdr([1.0; 3], Transfer::Hlg);
    assert!(white.iter().all(|&c| close(c, 0.75, 5e-3)), "{white:?}");
    assert_eq!(sdr_to_hdr([0.0; 3], Transfer::Hlg), [0.0; 3]);
}

#[test]
fn tone_curve_is_smooth_and_bounded() {
    for headroom in [1.0, 2.5, 16.0] {
        let knee = KNEE * headroom;
        assert_eq!(tone_curve(knee * 0.5, headroom), knee * 0.5);
        assert!(close(tone_curve(knee + 1e-4, headroom), knee + 1e-4, 1e-5));
        let mut last = 0.0;
        for i in 1..2000 {
            let y = tone_curve(i as f32 * 0.05, headroom);
            // Far above the knee it rounds to the headroom itself.
            assert!(y >= last && y <= headroom, "{headroom}: {y} after {last}");
            last = y;
        }
    }
}

#[test]
fn tone_map_keeps_hue() {
    let [r, g, b] = tone_map([2.0, 1.0, 0.5], 1.0);
    assert!(r < 1.0);
    assert!(close(g / r, 0.5, 1e-6) && close(b / r, 0.25, 1e-6));
}

#[test]
fn maps_hdr_greys_to_sdr() {
    let code = |signal: f32| (signal * 65535.0).round() as u16;
    for transfer in [Transfer::Pq, Transfer::Hlg] {
        let mapper = ToneMapper::new(transfer);
        assert_eq!(mapper.pixel([0; 3]), [0, 0, 0, 255]);
        // Reference white comes out just under SDR white...
        let white = match transfer {
            Transfer::Pq => 0.5806,
            Transfer::Hlg => 0.75,
        };
        let [b, g, r, _] = mapper.pixel([code(white); 3]);
        assert!(
            r == g && g == b && (235..=250).contains(&r),
            "{transfer:?}: {r}"
        );
        // ...and the brightest highlights just under the top, not clipped
        // far below it.
        let [b, ..] = mapper.pixel([u16::MAX; 3]);
        assert!((250..=255).contains(&b), "{transfer:?}: {b}");
    }
}

#[test]
fn applies_gamma_after_tone_mapping() {
    let mut mapper = ToneMapper::new(Transfer::Pq);
    let code = |signal: f32| (signal * 65535.0).round() as u16;
    // A grey a stop under reference white.
    let grey = [code(0.5); 3];
    let plain = mapper.pixel(grey)[1];
    mapper.set_gamma(1.5);
    assert!(mapper.pixel(grey)[1] > plain + 20);
    assert_eq!(mapper.pixel([0; 3]), [0, 0, 0, 255]);
    mapper.set_gamma(1.0);
    assert_eq!(mapper.pixel(grey)[1], plain);
}

/// Signals and the sRGB bytes they must come out as, worked out apart
/// from this code from the standards: ST 2084 for PQ, BT.2100 for HLG
/// with its OOTF on a 1000-nit display, BT.2087's matrix from BT.2020 to
/// BT.709, sRGB's encoding, and Plays' own tone curve above 0.75 of SDR
/// white. A byte either way is rounding.
#[test]
fn lands_known_signals_on_known_srgb() {
    let cases: &[(Transfer, [f32; 3], [u8; 3], &str)] = &[
        (Transfer::Pq, [0.25; 3], [44; 3], "5 nits"),
        (Transfer::Pq, [0.4; 3], [111; 3], "32 nits"),
        (Transfer::Pq, [0.5081; 3], [186; 3], "100 nits"),
        (
            Transfer::Pq,
            [0.5806; 3],
            [244; 3],
            "reference white, eased",
        ),
        (Transfer::Pq, [0.65; 3], [255; 3], "390 nits, white"),
        (Transfer::Hlg, [0.25; 3], [61; 3], "10 nits"),
        (Transfer::Hlg, [0.5; 3], [137; 3], "51 nits"),
        (Transfer::Hlg, [0.75; 3], [244; 3], "reference white, eased"),
        (Transfer::Hlg, [0.9; 3], [255; 3], "522 nits, white"),
        (
            Transfer::Pq,
            [0.46207, 0.27135, 0.17665],
            [186, 0, 0],
            "BT.709 red at 100 nits",
        ),
        (
            Transfer::Hlg,
            [0.29336, 0.4902, 0.15165],
            [0, 136, 0],
            "BT.709 green at 50 nits",
        ),
    ];
    let code = |signal: f32| (signal * 65535.0).round() as u16;
    for &(transfer, signal, expected, what) in cases {
        let [b, g, r, a] = ToneMapper::new(transfer).pixel(signal.map(code));
        let near = [r, g, b]
            .iter()
            .zip(expected)
            .all(|(&got, want)| got.abs_diff(want) <= 1);
        assert!(
            near && a == 255,
            "{transfer:?} {what}: {:?}, not {expected:?}",
            [r, g, b]
        );
    }
}

/// Gamma bends the sRGB signal after tone mapping: worked out as above.
#[test]
fn known_signals_with_gamma() {
    let code = |signal: f32| (signal * 65535.0).round() as u16;
    let cases = [
        (Transfer::Pq, 0.4, 1.5, 147),
        (Transfer::Pq, 0.4, 0.8, 90),
        (Transfer::Pq, 0.5806, 1.5, 248),
        (Transfer::Hlg, 0.5, 1.5, 168),
        (Transfer::Hlg, 0.5, 0.8, 117),
    ];
    for (transfer, signal, gamma, expected) in cases {
        let mut mapper = ToneMapper::new(transfer);
        mapper.set_gamma(gamma);
        let [_, g, ..] = mapper.pixel([code(signal); 3]);
        assert!(
            g.abs_diff(expected) <= 1,
            "{transfer:?} {signal} at gamma {gamma}: {g}, not {expected}"
        );
    }
}

#[test]
fn reads_pq_between_table_entries() {
    let mapper = ToneMapper::new(Transfer::Pq);
    for signal in [0.0, 0.1234, 0.5806, 0.9, 1.0] {
        let exact = pq_to_nits(signal) / REFERENCE_WHITE_NITS;
        let read = mapper.pq_light(signal);
        assert!(
            close(read, exact, exact * 1e-3 + 1e-7),
            "{signal}: {read} {exact}"
        );
    }
}

#[test]
fn runs_over_rows_with_padding() {
    let mapper = ToneMapper::new(Transfer::Pq);
    let (width, height, stride) = (3, 200, 3 * 6 + 10);
    let mut data = vec![0u8; stride * height];
    // The last pixel of the last row is PQ reference white.
    let white = ((0.5806f32 * 65535.0).round() as u16).to_le_bytes();
    let last = (height - 1) * stride + (width - 1) * 6;
    for i in 0..3 {
        data[last + 2 * i..][..2].copy_from_slice(&white);
    }
    let out = mapper.run(&data, stride, width, height);
    assert_eq!(out.len(), width * height * 4);
    assert_eq!(&out[..4], &[0, 0, 0, 255]);
    assert!(out[out.len() - 4] > 230);
}
