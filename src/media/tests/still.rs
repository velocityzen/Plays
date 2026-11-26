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
    use crate::media::tests::{block_differences, first_frame, hdr_media};

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
    ];
    for (name, path, transfer) in cases {
        let surface = first_frame(&path, true);
        assert!(matches!(surface, Picture::Surface(_)), "{name}: on the GPU");
        let Picture::Image(image) = first_frame(&path, false) else {
            panic!("{name}: software frames should be images");
        };
        let shows = shown(&surface, false).unwrap();
        let (width, height) = (shows.width as usize, shows.height as usize);
        let bgra = image.as_bytes(0).unwrap();
        assert_eq!(bgra.len(), width * height * 4, "{name}: sizes differ");
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
