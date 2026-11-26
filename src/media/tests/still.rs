use std::sync::Arc;

use ffmpeg::util::color;

use super::*;

/// R'G'B' (0 to 255) of 8-bit Y'CbCr samples by the matrix with the luma
/// weights `(kr, kb)`, in full range or not.
fn rgb((y, cb, cr): (f64, f64, f64), (kr, kb): (f64, f64), full_range: bool) -> [f64; 3] {
    let (y, chroma) = if full_range {
        (y, 1.0)
    } else {
        ((y - 16.0) * 255.0 / 219.0, 255.0 / 224.0)
    };
    let (cb, cr) = ((cb - 128.0) * chroma, (cr - 128.0) * chroma);
    let kg = 1.0 - kr - kb;
    [
        y + 2.0 * (1.0 - kr) * cr,
        y - 2.0 * kb * (1.0 - kb) / kg * cb - 2.0 * kr * (1.0 - kr) / kg * cr,
        y + 2.0 * (1.0 - kb) * cb,
    ]
}

/// The pixel in the middle of `still`, 0 to 255.
fn middle(still: &Still) -> [f64; 3] {
    let at = (still.height / 2 * still.width + still.width / 2) as usize;
    still.pixels[at].map(|c| f64::from(c) / 257.0)
}

#[test]
fn a_picture_in_memory_is_kept_as_it_shows() {
    let bgra = [
        [10, 20, 30, 255],
        [40, 50, 60, 255],
        [70, 80, 90, 255],
        [1, 2, 3, 255],
    ];
    let buffer = image::RgbaImage::from_raw(2, 2, bgra.concat()).unwrap();
    let image = RenderImage::new(vec![image::Frame::new(buffer)]);
    let still = shown(&Picture::Image(Arc::new(image)), true).unwrap();
    assert_eq!((still.width, still.height, still.hdr), (2, 2, None));
    let expected: Vec<[u16; 3]> = bgra
        .iter()
        .map(|&[b, g, r, _]| [r, g, b].map(|c| u16::from(c) * 257))
        .collect();
    assert_eq!(still.pixels, expected);
}

#[test]
fn planes_are_seen_in_the_colors_they_are_tagged_with() {
    let samples = (100u8, 150u8, 90u8);
    let (y, cb, cr) = samples;
    let size = (16, 16);
    let (luma, half) = (vec![y; 16 * 16], 8 * 8);
    let interleaved = [cb, cr].repeat(half);
    let (cbs, crs) = (vec![cb; half], vec![cr; half]);
    let cases = [
        (
            Pixel::NV12,
            vec![(&luma[..], 16), (&interleaved[..], 16)],
            color::Range::JPEG,
            color::Space::SMPTE170M,
            (0.299, 0.114),
        ),
        (
            Pixel::YUV420P,
            vec![(&luma[..], 16), (&cbs[..], 8), (&crs[..], 8)],
            color::Range::MPEG,
            color::Space::BT709,
            (0.2126, 0.0722),
        ),
    ];
    for (format, planes, range, space, weights) in cases {
        let colors = Colors {
            range,
            space,
            primaries: color::Primaries::Unspecified,
            transfer: color::TransferCharacteristic::Unspecified,
        };
        let frame = frame_of(format, size, &planes, colors).unwrap();
        assert_eq!(Colors::of_frame(&frame), colors, "{format:?} tagged");
        let still = of_frame(&frame, true).unwrap();
        assert_eq!((still.width, still.height, still.hdr), (16, 16, None));
        let samples = (f64::from(y), f64::from(cb), f64::from(cr));
        let expected = rgb(samples, weights, range == color::Range::JPEG);
        let seen = middle(&still);
        let off = (0..3)
            .map(|c| (seen[c] - expected[c]).abs())
            .fold(0.0, f64::max);
        assert!(off < 2.0, "{format:?}: {seen:?}, not {expected:?}");
    }
    let untagged = Colors {
        range: color::Range::Unspecified,
        space: color::Space::Unspecified,
        primaries: color::Primaries::Unspecified,
        transfer: color::TransferCharacteristic::Unspecified,
    };
    let short = [(&luma[..16], 16), (&interleaved[..], 16)];
    assert!(
        frame_of(Pixel::NV12, size, &short, untagged).is_err(),
        "a plane too short for its rows"
    );
    let missing = [(&luma[..], 16)];
    assert!(frame_of(Pixel::NV12, size, &missing, untagged).is_err());
}

#[test]
fn hdr_planes_are_kept_or_tone_mapped() {
    // Grey three quarters of the way to white, 10-bit video range, as
    // P010 keeps it: in the top bits.
    let (luma, chroma) = ((64 + 876 * 3 / 4) << 6, 512 << 6);
    let luma: Vec<u8> = [luma as u16; 16 * 16]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let chroma: Vec<u8> = [chroma as u16; 16 * 8]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let colors = Colors {
        range: color::Range::MPEG,
        space: color::Space::BT2020NCL,
        primaries: color::Primaries::BT2020,
        transfer: color::TransferCharacteristic::SMPTE2084,
    };
    let planes = [(&luma[..], 32), (&chroma[..], 32)];
    let frame = frame_of(Pixel::P010LE, (16, 16), &planes, colors).unwrap();

    let kept = of_frame(&frame, true).unwrap();
    assert_eq!(kept.hdr, Some(Transfer::Pq));
    let signal = middle(&kept).map(|c| c / 255.0);
    assert!(signal.iter().all(|s| (s - 0.75).abs() < 0.01), "{signal:?}");

    let mapped = of_frame(&frame, false).unwrap();
    assert_eq!(mapped.hdr, None, "tone mapped for an SDR screen");
}

/// Each channel of `still`'s pixel at `x`, `y`, 0 to 255.
#[cfg(target_os = "macos")]
fn levels(still: &Still) -> impl Fn(usize, usize, usize) -> f64 + '_ {
    let width = still.width as usize;
    move |x, y, c| f64::from(still.pixels[y * width + x][c]) / 257.0
}

/// Hardware-decoded frames stay on the GPU: read back, they're seen in the
/// colors they show in, as the same frame decoded in software, and HDR is
/// kept as the file has it.
#[cfg(target_os = "macos")]
#[test]
fn a_surface_is_seen_in_the_colors_it_shows() {
    use crate::media::tests::made::media;
    use crate::media::tests::{
        MASTERED_AT_1000_NITS, block_differences, converted_first_frame, first_frame, hdr_media,
        hdr_media_in,
    };

    let sdr = |name: &str, source: &str| {
        let args = [
            "-f", "lavfi", "-i", source, "-c:v", "libx264", "-crf", "8", "-pix_fmt", "yuv420p",
        ];
        media(name, &args)
    };
    let cases = [
        // Its bars are colors at their fullest, which a wrong range clips
        // as a right one does: the gradients' are in between.
        (
            "sd",
            sdr("colors-sd.mp4", "testsrc2=size=640x360:rate=30:duration=1"),
            None,
        ),
        (
            "gradients",
            sdr("gradients.mp4", "gradients=size=640x360:rate=30:duration=1"),
            None,
        ),
        (
            "hdr10",
            hdr_media("hdr10.mp4", "smpte2084"),
            Some(Transfer::Pq),
        ),
        (
            "hlg",
            hdr_media("hlg.mp4", "arib-std-b67"),
            Some(Transfer::Hlg),
        ),
        // Tone mapped to the peak it was mastered for.
        (
            "hdr10, mastered",
            hdr_media_in(
                "hdr10-mastered.mp4",
                "smpte2084",
                false,
                MASTERED_AT_1000_NITS,
            ),
            Some(Transfer::Pq),
        ),
    ];
    for (name, path, transfer) in cases {
        let surface = first_frame(&path, true);
        assert!(matches!(surface, Picture::Surface(_)), "{name}: on the GPU");
        let (width, height, bgra) = converted_first_frame(&path);
        let shows = shown(&surface, false).unwrap();
        assert_eq!(
            (shows.width as usize, shows.height as usize),
            (width, height),
            "{name}: sizes differ"
        );
        let software = |x: usize, y: usize, c: usize| f64::from(bgra[(y * width + x) * 4 + 2 - c]);
        let kept = shown(&surface, true).unwrap();
        assert_eq!(kept.hdr, transfer, "{name}: HDR kept as it is");
        let file = grab(&Source::File(path.clone()), None, 0, 0, true).unwrap();
        // As `surfaces_match_software_colors` compares them. Tone mapping
        // takes the brightest to white however they're read, as HDR kept
        // doesn't.
        for (what, (mean, p90)) in [
            (
                "shown",
                block_differences(width, height, levels(&shows), software),
            ),
            (
                "kept",
                block_differences(width, height, levels(&kept), levels(&file)),
            ),
        ] {
            assert!(
                mean < 1.5 && p90 < 3.0,
                "{name}, {what}: colors differ by {mean:.2} levels on average, {p90:.2} at the 90th percentile"
            );
        }
    }
}

/// Dolby Vision read back from a surface comes without its RPU, so its
/// reshaping comes apart: kept as PQ, or tone mapped as a player shows it.
#[cfg(target_os = "macos")]
#[test]
fn dolby_vision_read_back_is_reshaped_and_kept_or_tone_mapped() {
    use crate::media::hdr::{REFERENCE_WHITE_NITS, nits_to_pq, pq_to_nits};

    let reshaping = crate::media::dovi::tests::reshaping();
    // 2 × 2 of one IPT color, ten bits high in sixteen: past the knee,
    // where the peak it was mastered for tells.
    let (i, pt) = (0.9, 0.45);
    let code = |value: f32| (((value * 1024.0).round() as u16) << 6).to_le_bytes();
    let luma = [code(i); 4].concat();
    let chroma = [code(pt); 2].concat();
    let colors = Colors {
        range: color::Range::MPEG,
        space: color::Space::BT2020NCL,
        primaries: color::Primaries::BT2020,
        transfer: color::TransferCharacteristic::Unspecified,
    };
    let frame = frame_of(Pixel::P010LE, (2, 2), &[(&luma, 4), (&chroma, 4)], colors).unwrap();
    let signal = |value: f32| f32::from(u16::from_le_bytes(code(value))) / 65536.0;
    let light = reshaping.lms_to_rgb(
        reshaping
            .lms_signal([signal(i), signal(pt), signal(pt)])
            .map(pq_to_nits),
    );

    let kept = reshaped(&frame, &reshaping, true).unwrap();
    assert_eq!(kept.hdr, Some(Transfer::Pq));
    for (c, nits) in light.iter().enumerate() {
        let got = f32::from(kept.pixels[0][c]) / 65535.0;
        assert!((got - nits_to_pq(*nits)).abs() < 1e-3, "{c}: {got}");
    }

    let shows = reshaped(&frame, &reshaping, false).unwrap();
    assert_eq!(shows.hdr, None);
    let mut mapper = ToneMapper::new(Transfer::Pq);
    mapper.set_gamma(crate::media::picture::gamma(true));
    // To the peak of the display it was mastered on.
    mapper.set_peak(reshaping.peak_nits());
    let [b, g, r, _] = mapper.output(light.map(|nits| nits / REFERENCE_WHITE_NITS));
    for (c, want) in [r, g, b].into_iter().enumerate() {
        let got = shows.pixels[0][c] / 257;
        assert!(got.abs_diff(u16::from(want)) <= 1, "{c}: {got} for {want}");
    }
}

/// A screenshot of Dolby Vision profile 7 is composed, its enhancement
/// layer in a track of its own, in a time base of its own, or in the
/// video's; and, with none there, it's of the base layer as it is.
#[test]
fn a_screenshot_of_dolby_vision_profile_7_is_composed() {
    use crate::media::hdr::Transfer;
    use crate::media::tests::{Layout, green, plain_green, profile_7_media};

    let plain = plain_green();
    for (layout, stream, composed) in [
        (Layout::TwoTracks, 1, true),
        (Layout::OneTrack, 0, true),
        (Layout::Missing, 0, false),
    ] {
        let path = profile_7_media(layout);
        let input = ffmpeg::format::input(&path).unwrap();
        let time_base = input.stream(stream).unwrap().time_base();
        drop(input);
        // Half a second in, the sixth frame.
        let timestamp = (0.5 / f64::from(time_base)).round() as i64;
        let still = grab(&Source::File(path), None, stream, timestamp, true).unwrap();
        assert_eq!((still.width, still.height), (320, 180), "{layout:?}");
        assert_eq!(still.hdr, Some(Transfer::Pq), "{layout:?}");
        let raised = green(&still) - plain;
        assert_eq!(raised > 0.1, composed, "{layout:?}: {raised:.3} greener");
    }
}

/// A screenshot is the shape its pixels are, as its pictures say or, over
/// that, its container: 4:3 pixels of 160×120 as H.264 has them, and the
/// 3:2 of a Matroska file saying it's twice as wide as it's high.
#[test]
fn a_screenshot_is_the_shape_of_its_pixels() {
    use crate::media::tests::made::media;
    let wide = media(
        "still-sar.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=1",
            "-vf",
            "setsar=4/3",
            "-c:v",
            "libx264",
        ],
    );
    let told = media(
        "still-aspect.mkv",
        &["-i", wide.to_str().unwrap(), "-c", "copy", "-aspect", "2:1"],
    );
    for (path, size) in [(wide, (212, 120)), (told, (240, 120))] {
        let still = grab(&Source::File(path.clone()), None, 0, 0, false).unwrap();
        assert_eq!((still.width, still.height), size, "{}", path.display());
    }
}
