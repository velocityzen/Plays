use std::mem::offset_of;

use ffmpeg::util::format::Pixel;

use super::*;

fn close(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

/// A frame encoded with `transfer`, with side data of a MaxCLL and of a
/// mastering display's peak (`num` / `den` nits, and whether it says), each
/// if given.
fn frame_with(
    transfer: TransferCharacteristic,
    max_cll: Option<u32>,
    display: Option<(i32, i32, bool)>,
) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::YUV420P10LE, 2, 2);
    frame.set_color_transfer_characteristic(transfer);
    attach_hdr10(&mut frame, max_cll, display);
    frame
}

/// Give `frame` side data of a MaxCLL and of a mastering display's peak,
/// each if given, as `frame_with` does.
pub(crate) fn attach_hdr10(
    frame: &mut frame::Video,
    max_cll: Option<u32>,
    display: Option<(i32, i32, bool)>,
) {
    let mut side = |kind, size| {
        // SAFETY: FFmpeg makes side data of the size asked for, or none.
        let side = unsafe { ffi::av_frame_new_side_data(frame.as_mut_ptr(), kind, size) };
        assert!(!side.is_null());
        // SAFETY: as above.
        unsafe { (*side).data }
    };
    if let Some(max_cll) = max_cll {
        let data = side(
            ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL,
            size_of::<sys::ContentLightLevel>(),
        );
        // SAFETY: the side data is as big as the struct, and aligned.
        unsafe {
            data.cast::<sys::ContentLightLevel>()
                .write(sys::ContentLightLevel {
                    max_cll,
                    max_fall: max_cll / 2,
                });
        }
    }
    if let Some((num, den, has_luminance)) = display {
        let data = side(
            ffi::AVFrameSideDataType::AV_FRAME_DATA_MASTERING_DISPLAY_METADATA,
            size_of::<sys::MasteringDisplay>(),
        );
        // SAFETY: as above; all-zero is a valid display otherwise.
        unsafe {
            let mut mastering: sys::MasteringDisplay = std::mem::zeroed();
            mastering.max_luminance = ffi::AVRational { num, den };
            mastering.min_luminance = ffi::AVRational {
                num: 50,
                den: 10_000,
            };
            mastering.has_luminance = has_luminance.into();
            data.cast::<sys::MasteringDisplay>().write(mastering);
        }
    }
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
        for peak in [None, Some(headroom * 0.5), Some(headroom * 1.2), Some(49.3)] {
            assert_eq!(tone_curve(knee * 0.5, headroom, peak), knee * 0.5);
            let past = tone_curve(knee + 1e-4, headroom, peak);
            assert!(close(past, knee + 1e-4, 1e-5), "{headroom}, {peak:?}");
            let mut last = 0.0;
            for i in 1..2000 {
                let y = tone_curve(i as f32 * 0.05, headroom, peak);
                // Far above the knee it rounds to the headroom itself.
                assert!(
                    y >= last && y <= headroom,
                    "{headroom}, {peak:?}: {y} after {last}"
                );
                last = y;
            }
        }
    }
}

/// Points on the curve, worked out apart from this code from its
/// definition: on an SDR screen, and on one with 4 times SDR white's
/// headroom, for videos mastered at 1,000 and 4,000 nits.
#[test]
fn a_known_peak_lands_where_its_curve_goes() {
    let nits = |nits: f32| nits / REFERENCE_WHITE_NITS;
    let cases = [
        (nits(406.0), 1.0, Some(nits(1000.0)), 0.962_066),
        (nits(812.0), 4.0, Some(nits(1000.0)), 3.634_775),
        (nits(1000.0), 4.0, Some(nits(1000.0)), 4.0),
        (nits(2030.0), 4.0, Some(nits(1000.0)), 4.0),
        (nits(812.0), 4.0, Some(nits(4000.0)), 3.501_792),
        (nits(1000.0), 4.0, Some(nits(4000.0)), 3.662_793),
        (nits(2030.0), 4.0, Some(nits(4000.0)), 3.896_95),
        // One that fits is left alone.
        (nits(700.0), 4.0, Some(nits(800.0)), nits(700.0)),
    ];
    for (x, headroom, peak, expected) in cases {
        let y = tone_curve(x, headroom, peak);
        assert!(close(y, expected, 1e-5), "{x} under {headroom}: {y}");
    }
    // Without a peak, it never quite gets there.
    assert!(tone_curve(nits(1000.0), 4.0, None) < 3.86);
}

#[test]
fn tone_map_keeps_hue() {
    for peak in [None, Some(4.9)] {
        let [r, g, b] = tone_map([2.0, 1.0, 0.5], 1.0, peak);
        assert!(r < 1.0);
        assert!(close(g / r, 0.5, 1e-6) && close(b / r, 0.25, 1e-6));
    }
}

#[test]
fn mirrors_ffmpegs_layout() {
    // From offsetof() against libavutil/mastering_display_metadata.h.
    assert_eq!(offset_of!(sys::MasteringDisplay, white_point), 48);
    assert_eq!(offset_of!(sys::MasteringDisplay, max_luminance), 72);
    assert_eq!(offset_of!(sys::MasteringDisplay, has_luminance), 84);
    assert_eq!(size_of::<sys::MasteringDisplay>(), 88);
    assert_eq!(offset_of!(sys::ContentLightLevel, max_fall), 4);
    assert_eq!(size_of::<sys::ContentLightLevel>(), 8);
}

#[test]
fn a_pq_videos_peak_is_the_lower_of_what_its_metadata_says() {
    let thousand = Some((10_000_000, 10_000, true));
    let cases = [
        (None, None, None, "nothing said"),
        (Some(1000), None, Some(1000.0), "MaxCLL"),
        (
            None,
            Some((40_000_000, 10_000, true)),
            Some(4000.0),
            "the display",
        ),
        (Some(600), thousand, Some(600.0), "MaxCLL under the display"),
        (
            Some(2000),
            thousand,
            Some(1000.0),
            "the display under MaxCLL",
        ),
        (Some(0), thousand, Some(1000.0), "MaxCLL unsaid"),
        (
            None,
            Some((10_000_000, 10_000, false)),
            None,
            "the display's peak unsaid",
        ),
        (
            None,
            Some((1000, 0, true)),
            None,
            "the display's peak over nothing",
        ),
        (Some(100), None, None, "under reference white"),
        (
            None,
            Some((200_000_000, 10_000, true)),
            None,
            "over PQ's peak",
        ),
        (
            Some(100),
            thousand,
            Some(1000.0),
            "MaxCLL under reference white",
        ),
    ];
    for (max_cll, display, expected, what) in cases {
        let frame = frame_with(TransferCharacteristic::SMPTE2084, max_cll, display);
        assert_eq!(peak_nits(&frame), expected, "{what}");
    }
    // HLG is shown as on its nominal display, whatever it says; SDR has none.
    let hlg = frame_with(TransferCharacteristic::ARIB_STD_B67, Some(4000), None);
    assert_eq!(peak_nits(&hlg), Some(HLG_PEAK_NITS));
    let sdr = frame_with(TransferCharacteristic::BT709, Some(1000), thousand);
    assert_eq!(peak_nits(&sdr), None);
}

#[test]
fn a_peak_attached_is_found_again() {
    let mut frame = frame_with(TransferCharacteristic::SMPTE2084, None, None);
    attach_peak(&mut frame, 812.4).unwrap();
    assert_eq!(peak_nits(&frame), Some(812.0));
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

/// Side data shorter than FFmpeg's struct for its kind isn't read past its
/// end: it says no peak.
#[test]
fn side_data_too_short_for_its_kind_says_nothing() {
    let mut frame = frame_with(TransferCharacteristic::SMPTE2084, None, None);
    let kind = ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL;
    // SAFETY: FFmpeg makes side data of the size asked for, or none; its
    // data is aligned for a u32.
    unsafe {
        let side = ffi::av_frame_new_side_data(frame.as_mut_ptr(), kind, 4);
        assert!(!side.is_null());
        (*side).data.cast::<u32>().write(1000);
    }
    assert_eq!(peak_nits(&frame), None);
}

/// Highlights up to a video's peak stay apart, where without it they'd
/// all be white: worked out as below, for videos mastered at 300, 1,000
/// and 4,000 nits.
#[test]
fn highlights_stay_apart_up_to_a_known_peak() {
    let code = |signal: f32| (signal * 65535.0).round() as u16;
    let cases = [
        (None, 0.649_86, 255, "390 nits, unknown peak"),
        (Some(1000.0), 0.580_69, 240, "reference white"),
        (Some(1000.0), 0.649_86, 250, "390 nits"),
        (Some(1000.0), 0.696_29, 253, "600 nits"),
        (Some(1000.0), 0.751_83, 255, "1,000 nits, the peak"),
        (Some(4000.0), 0.751_83, 253, "1,000 nits, under the peak"),
        (Some(4000.0), 0.902_57, 255, "4,000 nits, the peak"),
        (Some(300.0), 0.621_86, 255, "300 nits, a dim master's peak"),
    ];
    for (peak, signal, expected, what) in cases {
        let mut mapper = ToneMapper::new(Transfer::Pq);
        mapper.set_peak(peak);
        let [_, g, ..] = mapper.pixel([code(signal); 3]);
        assert!(
            g.abs_diff(expected) <= 1,
            "{what} of {peak:?}: {g}, not {expected}"
        );
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
