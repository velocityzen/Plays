use ffmpeg_next::ffi;
use ffmpeg_next::util::color::TransferCharacteristic;
use ffmpeg_next::util::format::Pixel;

use super::*;
use crate::testing::attach_hdr10;

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

#[test]
fn tone_curve_is_smooth_and_bounded() {
    for headroom in [1.0, 2.5, 16.0] {
        let knee = KNEE * headroom;
        for peak in [None, Some(headroom * 0.5), Some(headroom * 1.2), Some(49.3)] {
            assert_eq!(tone_curve(knee * 0.5, headroom, peak), knee * 0.5);
            assert_eq!(tone_curve(knee, headroom, peak), knee);
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
        (nits(1000.0), 1.0, Some(nits(1000.0)), 1.0),
        (nits(2000.0), 1.0, Some(nits(1000.0)), 1.0),
        (nits(812.0), 4.0, Some(nits(1000.0)), 3.634_775),
        (nits(1000.0), 4.0, Some(nits(1000.0)), 4.0),
        (nits(2030.0), 4.0, Some(nits(1000.0)), 4.0),
        (nits(812.0), 4.0, Some(nits(4000.0)), 3.501_792),
        (nits(1000.0), 4.0, Some(nits(4000.0)), 3.662_793),
        (nits(2030.0), 4.0, Some(nits(4000.0)), 3.896_95),
        // One that fits is left alone.
        (nits(700.0), 4.0, Some(nits(800.0)), nits(700.0)),
        (0.9, 1.0, Some(0.95), 0.9),
    ];
    for (x, headroom, peak, expected) in cases {
        let y = tone_curve(x, headroom, peak);
        assert!(close(y, expected, 1e-5), "{x} under {headroom}: {y}");
    }
    // Without a peak, it nears the headroom, short of it.
    assert!(tone_curve(nits(1000.0), 4.0, None) < 3.86);
    let unknown = tone_curve(2.0, 1.0, None);
    assert!(unknown < 1.0 && unknown > 0.99, "{unknown}");
}

#[test]
fn tone_map_keeps_hue() {
    for peak in [None, Some(4.9)] {
        let [r, g, b] = tone_map([2.0, 1.0, 0.5], 1.0, peak);
        assert!(r < 1.0);
        assert!(close(g / r, 0.5, 1e-6) && close(b / r, 0.25, 1e-6));
    }
    assert_eq!(tone_map([0.0, -0.1, 0.0], 1.0, None), [0.0, -0.1, 0.0]);
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
        (Some(203), None, Some(203.0), "at reference white"),
        (Some(10000), None, Some(10000.0), "at PQ's peak"),
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
