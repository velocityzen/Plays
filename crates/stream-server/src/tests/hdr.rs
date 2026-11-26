use super::*;

/// A flat grey 64 × 32 10-bit picture in limited range, its luma `signal`
/// (0 to 1), tagged BT.2020 with `transfer` and `space`.
pub(crate) fn grey(
    transfer: color::TransferCharacteristic,
    space: color::Space,
    signal: f64,
) -> frame::Video {
    flat(transfer, space, [signal, 0.0, 0.0])
}

/// A 64 × 32 10-bit picture in limited range of one color all over: its
/// luma (0 to 1), and blue and red differences (-0.5 to 0.5), tagged
/// BT.2020 with `transfer` and `space`.
fn flat(
    transfer: color::TransferCharacteristic,
    space: color::Space,
    [y, cb, cr]: [f64; 3],
) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::YUV420P10LE, 64, 32);
    frame.set_color_primaries(color::Primaries::BT2020);
    frame.set_color_transfer_characteristic(transfer);
    frame.set_color_space(space);
    frame.set_color_range(color::Range::MPEG);
    let code = |value: f64, offset: f64, range: f64| (offset + value * range).round() as u16;
    for (plane, value) in [
        (0, code(y, 64.0, 876.0)),
        (1, code(cb, 512.0, 896.0)),
        (2, code(cr, 512.0, 896.0)),
    ] {
        let (stride, width) = (frame.stride(plane), frame.plane_width(plane) as usize);
        let rows = frame.plane_height(plane) as usize;
        for row in frame.data_mut(plane).chunks_mut(stride).take(rows) {
            for sample in row[..width * 2].as_chunks_mut::<2>().0 {
                *sample = value.to_le_bytes();
            }
        }
    }
    frame
}

/// The luma in the middle of an NV12 or P010 frame, 0 to 1 over limited
/// range.
pub(crate) fn luma(frame: &frame::Video) -> f64 {
    let row = frame.stride(0) * (frame.height() as usize / 2);
    let data = frame.data(0);
    match frame.format() {
        Pixel::NV12 => (f64::from(data[row + frame.width() as usize / 2]) - 16.0) / 219.0,
        _ => {
            let at = row + frame.width() as usize;
            (f64::from(u16::from_le_bytes([data[at], data[at + 1]]) >> 6) - 64.0) / 876.0
        }
    }
}

/// The luma, and blue and red differences, in the middle of an NV12 or
/// P010 frame, as `flat` takes them.
fn ycbcr(frame: &frame::Video) -> [f64; 3] {
    let (x, y) = (frame.width() as usize / 2, frame.height() as usize / 2);
    let chroma = frame.stride(1) * (y / 2);
    let (luma, data) = (luma(frame), frame.data(1));
    let [cb, cr] = match frame.format() {
        Pixel::NV12 => [0, 1].map(|i| (f64::from(data[chroma + x + i]) - 128.0) / 224.0),
        _ => [0, 2].map(|i| {
            let at = chroma + 2 * x + i;
            (f64::from(u16::from_le_bytes([data[at], data[at + 1]]) >> 6) - 512.0) / 896.0
        }),
    };
    [luma, cb, cr]
}

/// BT.709's tags, in limited range.
pub(crate) fn sdr_tags() -> Tags {
    Tags {
        primaries: ffi::AVColorPrimaries::AVCOL_PRI_BT709 as u32,
        transfer: ffi::AVColorTransferCharacteristic::AVCOL_TRC_BT709 as u32,
        space: ffi::AVColorSpace::AVCOL_SPC_BT709 as u32,
        range: ffi::AVColorRange::AVCOL_RANGE_MPEG as u32,
    }
}

/// PQ's code for `nits`.
pub(crate) fn pq(nits: f64) -> f64 {
    let (m1, m2) = (f64::from(PQ_M1), f64::from(PQ_M2));
    let (c1, c2, c3) = (f64::from(PQ_C1), f64::from(PQ_C2), f64::from(PQ_C3));
    let y = (nits / 10000.0).powf(m1);
    ((c1 + c2 * y) / (1.0 + c3 * y)).powf(m2)
}

/// Gives `frame` the MaxCLL `nits`.
fn with_peak(frame: &mut frame::Video, nits: u32) {
    // SAFETY: the side data FFmpeg makes for the frame is as big as asked,
    // and this kind is `ContentLightLevel`.
    unsafe {
        let side = ffi::av_frame_new_side_data(
            frame.as_mut_ptr(),
            ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL,
            size_of::<sys::ContentLightLevel>(),
        );
        assert!(!side.is_null());
        (*side)
            .data
            .cast::<sys::ContentLightLevel>()
            .write(sys::ContentLightLevel {
                max_cll: nits,
                max_fall: 0,
            });
    }
}

/// `from` fitted to SDR in 32 × 16 `format`.
fn fitted(from: &frame::Video, format: Pixel) -> frame::Video {
    let transfer = Transfer::of(from).expect("HDR");
    let mut to = frame::Video::new(format, 32, 16);
    Converter::default()
        .run(from, Conversion::ToSdr(transfer), &mut to)
        .unwrap();
    to
}

/// SDR's light, 0 to 1, of a luma a TV shows (BT.1886).
fn shown(luma: f64) -> f64 {
    luma.max(0.0).powf(f64::from(TV_GAMMA))
}

#[test]
fn leaves_brightness_below_the_knee_alone_and_rolls_off_toward_white() {
    for x in [0.0, 0.1, 0.5, KNEE] {
        assert_eq!(tone_curve(x, Some(4.9)), x);
        assert_eq!(tone_curve(x, None), x);
    }
    // A video peaking at its mastering display's 1000 nits reaches white
    // there, and no further.
    let peak = 1000.0 / REFERENCE_WHITE_NITS;
    assert!((tone_curve(peak, Some(peak)) - 1.0).abs() < 1e-6);
    assert_eq!(tone_curve(peak * 2.0, Some(peak)), 1.0);
    // Without a peak, it nears white, short of it.
    assert!(tone_curve(2.0, None) < 1.0);
    assert!(tone_curve(2.0, None) > 0.99);
    // A video that fits is left alone.
    assert_eq!(tone_curve(0.9, Some(0.95)), 0.9);
    // It never darkens what's brighter.
    let mut last = 0.0;
    for step in 0..2000 {
        let x = step as f32 * 0.01;
        let y = tone_curve(x, Some(peak));
        assert!(y >= last, "{x}: {y} after {last}");
        last = y;
    }
}

/// HDR's reference white is SDR's white, nearly: just under it, as the
/// curve rolls off above three quarters of it. Its codes taken for
/// BT.709's, as they were, show a dim grey.
#[test]
fn shows_hdr_reference_white_nearly_as_sdr_white() {
    let pq_white = grey(
        color::TransferCharacteristic::SMPTE2084,
        color::Space::BT2020NCL,
        pq(203.0),
    );
    // 75% HLG is reference white too.
    let hlg_white = grey(
        color::TransferCharacteristic::ARIB_STD_B67,
        color::Space::BT2020NCL,
        0.75,
    );
    for format in [Pixel::NV12, Pixel::P010LE] {
        for white in [&pq_white, &hlg_white] {
            let fitted = fitted(white, format);
            let light = shown(luma(&fitted));
            assert!((0.8..0.95).contains(&light), "{format:?}: {light}");
            assert!(shown(luma(&fitted)) > 3.0 * shown(0.58), "{format:?}");
            // Grey stays grey.
            let chroma = fitted.data(1)[fitted.stride(1) * 4 + 8];
            if format == Pixel::NV12 {
                assert!(chroma.abs_diff(128) <= 1, "{chroma}");
            }
        }
    }
    assert_eq!(Tags::of(&fitted(&pq_white, Pixel::NV12)), sdr_tags());
}

/// Light under the knee is shown as bright as it was mastered: 100 nits is
/// half of SDR's white. A highlight at the video's peak is white, and one
/// that peaks higher is rolled off further.
#[test]
fn fits_highlights_by_the_videos_peak() {
    let grey_at = |nits| {
        grey(
            color::TransferCharacteristic::SMPTE2084,
            color::Space::BT2020NCL,
            pq(nits),
        )
    };
    let half = shown(luma(&fitted(&grey_at(100.0), Pixel::P010LE)));
    assert!((half - 100.0 / 203.0).abs() < 0.02, "{half}");

    let mut peak = grey_at(1000.0);
    with_peak(&mut peak, 1000);
    let white = shown(luma(&fitted(&peak, Pixel::P010LE)));
    assert!(white > 0.98, "{white}");
    let mut brighter = grey_at(600.0);
    with_peak(&mut brighter, 1000);
    let mut brightest = grey_at(600.0);
    with_peak(&mut brightest, 4000);
    let (rolled, more) = (
        shown(luma(&fitted(&brighter, Pixel::P010LE))),
        shown(luma(&fitted(&brightest, Pixel::P010LE))),
    );
    assert!(more < rolled && rolled < white, "{more} {rolled} {white}");
}

#[test]
fn reads_hdr_by_its_transfer_in_bt2020_only() {
    use color::{Space, TransferCharacteristic as Trc};
    let of = |transfer, space| Transfer::of(&grey(transfer, space, 0.5));
    assert_eq!(of(Trc::SMPTE2084, Space::BT2020NCL), Some(Transfer::Pq));
    assert_eq!(of(Trc::ARIB_STD_B67, Space::BT2020NCL), Some(Transfer::Hlg));
    assert_eq!(of(Trc::SMPTE2084, Space::Unspecified), Some(Transfer::Pq));
    assert_eq!(of(Trc::BT709, Space::BT2020NCL), None);
    assert_eq!(of(Trc::BT2020_10, Space::BT2020NCL), None);
    // IPT, as Dolby Vision's profile 20 is tagged, isn't YCbCr to fit.
    let mut ipt = grey(Trc::SMPTE2084, Space::BT2020NCL, 0.5);
    // SAFETY: the frame is ours, and IPT-C2 a value FFmpeg names.
    unsafe { (*ipt.as_mut_ptr()).colorspace = ffi::AVColorSpace::AVCOL_SPC_IPT_C2 };
    assert_eq!(Transfer::of(&ipt), None);
}

#[test]
fn reads_the_peak_as_the_lower_of_maxcll_and_the_mastering_display() {
    let mut frame = grey(
        color::TransferCharacteristic::SMPTE2084,
        color::Space::BT2020NCL,
        0.5,
    );
    assert_eq!(peak_nits(&frame, Transfer::Pq), None);
    assert_eq!(peak_nits(&frame, Transfer::Hlg), Some(HLG_PEAK_NITS));
    with_peak(&mut frame, 2010);
    assert_eq!(peak_nits(&frame, Transfer::Pq), Some(2010.0));
    // SAFETY: the side data FFmpeg makes for the frame is as big as asked,
    // and this kind is `MasteringDisplay`.
    unsafe {
        let side = ffi::av_frame_new_side_data(
            frame.as_mut_ptr(),
            ffi::AVFrameSideDataType::AV_FRAME_DATA_MASTERING_DISPLAY_METADATA,
            size_of::<sys::MasteringDisplay>(),
        );
        assert!(!side.is_null());
        let display = (*side).data.cast::<sys::MasteringDisplay>();
        std::ptr::write_bytes(display, 0, 1);
        (*display).max_luminance = ffi::AVRational { num: 1000, den: 1 };
        (*display).has_luminance = 1;
    }
    assert_eq!(peak_nits(&frame, Transfer::Pq), Some(1000.0));
}

/// Dolby Vision's IPT at HDR's reference white, a frame `dovi`'s tests
/// make: its chroma neutral, its luma code the one this frame's curve
/// reshapes to PQ's code for 203 nits.
fn ipt_white() -> frame::Video {
    use crate::dovi::tests::{NEUTRAL, rpu_frame};
    let neutral = (NEUTRAL * 1024.0).round() as u16;
    rpu_frame([833, neutral, neutral])
}

/// Reshaped into HDR10, IPT keeps its light: reference white has PQ's code
/// for 203 nits, grey and in BT.2020. Fitted to SDR instead, it's as HDR10's
/// reference white is, nearly SDR's white, by the same peak: the 1000 nits
/// its RPU says it was mastered at.
#[test]
fn reshapes_ipt_into_hdr10_as_it_was_mastered_or_fits_it_to_sdr() {
    let white = ipt_white();
    let reshaping = Reshaping::of(&white).unwrap();
    let converted = |format, hdr10| {
        let mut to = frame::Video::new(format, 32, 16);
        Converter::default()
            .run(
                &white,
                Conversion::Reshaped {
                    reshaping: &reshaping,
                    hdr10,
                },
                &mut to,
            )
            .unwrap();
        to
    };

    let hdr10 = converted(Pixel::P010LE, true);
    assert!((luma(&hdr10) - pq(203.0)).abs() < 0.003, "{}", luma(&hdr10));
    let at = hdr10.stride(1) * 4 + 16;
    let chroma = &hdr10.data(1)[at..at + 4];
    for sample in [[chroma[0], chroma[1]], [chroma[2], chroma[3]]] {
        let code = u16::from_le_bytes(sample) >> 6;
        assert!(code.abs_diff(512) <= 1, "{code}");
    }
    assert_eq!(
        Tags::of(&hdr10),
        Tags {
            primaries: ffi::AVColorPrimaries::AVCOL_PRI_BT2020 as u32,
            transfer: ffi::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE2084 as u32,
            space: ffi::AVColorSpace::AVCOL_SPC_BT2020_NCL as u32,
            range: ffi::AVColorRange::AVCOL_RANGE_MPEG as u32,
        }
    );

    let mut hdr10_white = grey(
        color::TransferCharacteristic::SMPTE2084,
        color::Space::BT2020NCL,
        pq(203.0),
    );
    with_peak(&mut hdr10_white, 1000);
    for format in [Pixel::NV12, Pixel::P010LE] {
        let sdr = converted(format, false);
        let as_hdr10 = fitted(&hdr10_white, format);
        assert!(
            (shown(luma(&sdr)) - shown(luma(&as_hdr10))).abs() < 0.02,
            "{format:?}: {} {}",
            luma(&sdr),
            luma(&as_hdr10)
        );
        assert_eq!(Tags::of(&sdr), sdr_tags());
    }
}

/// PQ's codes come from a table, by the fourth root of the light, finely
/// enough from a mastering display's black to PQ's peak that 12 bits of
/// them are right.
#[test]
fn encodes_pq_finely_from_black_to_its_peak() {
    let table = Tables::default().pq_encode;
    for nits in [0.005, 0.05, 0.5, 5.0, 50.0, 203.0, 1000.0, 4000.0, 10000.0] {
        let root = (nits / PQ_PEAK_NITS).sqrt().sqrt();
        let coded = f64::from(table[(root * ENCODE_STEPS as f32 + 0.5) as usize]) / 65535.0;
        let exact = pq(f64::from(nits));
        assert!(
            (coded - exact).abs() < 1.0 / 4096.0,
            "{nits}: {coded} {exact}"
        );
        assert!((f64::from(nits_to_pq(nits)) - exact).abs() < 1e-5, "{nits}");
    }
}

/// HDR10's colors are read by BT.2020's matrix, not BT.601's that swscale
/// takes untagged YCbCr in, and coded by BT.709's: its code for BT.709's
/// (30, 60, 15) nits is SDR's for the same color, below where the curve
/// rolls off. Worked out apart from this code, through BT.2087, ST 2084
/// and BT.2020's matrix to 10-bit codes, then back and through BT.1886 and
/// BT.709's.
#[test]
fn fits_hdr10s_colors_by_their_matrices() {
    let color = flat(
        color::TransferCharacteristic::SMPTE2084,
        color::Space::BT2020NCL,
        [0.4384, -0.0446, -0.0145],
    );
    for (format, close) in [(Pixel::P010LE, 0.008), (Pixel::NV12, 0.01)] {
        let got = ycbcr(&fitted(&color, format));
        let want = [0.5511, -0.1141, -0.0658];
        assert!(
            want.iter().zip(got).all(|(a, b)| (a - b).abs() < close),
            "{format:?}: want {want:?}, got {got:?}"
        );
    }
}
