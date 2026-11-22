use super::*;

const SDR: Coding = Coding {
    kr: 0.2126,
    kb: 0.0722,
    transfer: Transfer::Sdr,
};

fn bar(x: i32, y: i32, width: usize, height: usize) -> Picture {
    // White, outlined in black, on nothing.
    let indices = (0..height)
        .flat_map(|row| {
            (0..width).map(move |column| {
                if row == 0 || column == 0 || row == height - 1 || column == width - 1 {
                    2
                } else {
                    1
                }
            })
        })
        .collect();
    Picture {
        x,
        y,
        width,
        height,
        indices,
        palette: vec![0x0000_0000, 0xFFFF_FFFF, 0xFF00_0000],
    }
}

fn sets() -> Sets {
    let mut sets = Sets::default();
    sets.add(0.2, Some(1.0), vec![bar(0, 0, 4, 4)], (8, 8));
    sets.add(1.2, None, vec![bar(4, 4, 4, 4)], (8, 8));
    sets.add(2.0, None, Vec::new(), (8, 8));
    sets
}

#[test]
fn shows_each_set_from_its_start_to_its_end_or_the_next() {
    let mut sets = sets();
    let showing = |sets: &mut Sets, time| sets.at(time).map(|set| set.id);
    assert_eq!(showing(&mut sets, 0.0), None);
    assert_eq!(showing(&mut sets, 0.2), Some(1));
    assert_eq!(showing(&mut sets, 0.96), Some(1));
    assert_eq!(showing(&mut sets, 1.0), None, "it ends");
    assert_eq!(showing(&mut sets, 1.2), Some(2));
    assert_eq!(showing(&mut sets, 1.96), Some(2), "it has no end");
    assert_eq!(showing(&mut sets, 2.0), None, "an empty set clears it");
    assert_eq!(sets.sets.len(), 1, "sets replaced go");
}

#[test]
fn keeps_sets_in_order_however_they_come() {
    let mut sets = Sets::default();
    sets.add(1.2, None, vec![bar(0, 0, 2, 2)], (8, 8));
    sets.add(0.2, None, vec![bar(0, 0, 2, 2)], (8, 8));
    sets.add(0.7, None, vec![bar(0, 0, 2, 2)], (8, 8));
    assert_eq!(sets.at(0.5).map(|set| set.id), Some(2));
    assert_eq!(sets.at(0.8).map(|set| set.id), Some(3));
    assert_eq!(sets.at(1.5).map(|set| set.id), Some(1));
}

#[test]
fn converts_palette_colors_premultiplied() {
    let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-3);
    assert!(close(SDR.color(0xFFFF_FFFF), [1.0, 0.0, 0.0, 1.0]));
    assert!(close(SDR.color(0xFF00_0000), [0.0, 0.0, 0.0, 1.0]));
    assert!(close(SDR.color(0x80FF_FFFF), [0.502, 0.0, 0.0, 0.502]));
    assert!(close(SDR.color(0x0012_3456), [0.0; 4]));
    let red = SDR.color(0xFFFF_0000);
    assert!((red[0] - 0.2126).abs() < 1e-3 && (red[2] - 0.5).abs() < 1e-3);
    // HDR's white is graphics white, not the brightest.
    let pq = Coding {
        transfer: Transfer::Pq,
        ..SDR
    };
    assert!((pq.color(0xFFFF_FFFF)[0] - 0.58).abs() < 0.01);
    let hlg = Coding {
        transfer: Transfer::Hlg,
        ..SDR
    };
    assert!((hlg.color(0xFFFF_FFFF)[0] - 0.75).abs() < 0.01);
}

#[test]
fn copies_a_picture_at_the_canvas_size() {
    let patch = patch(&bar(2, 1, 4, 3), (8, 8), (8, 8), &SDR).unwrap();
    // A pixel of nothing either side.
    assert_eq!((patch.x, patch.y), (1, 0));
    assert_eq!((patch.width, patch.height), (6, 5));
    let alpha: Vec<f32> = patch.pixels.iter().map(|pixel| pixel[3]).collect();
    assert_eq!(&alpha[..6], &[0.0; 6]);
    assert_eq!(&alpha[6..12], &[0.0, 1.0, 1.0, 1.0, 1.0, 0.0]);
    let luma: Vec<f32> = patch.pixels[12..18].iter().map(|pixel| pixel[0]).collect();
    assert_eq!(luma, [0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
}

#[test]
fn scales_a_picture_with_its_canvas() {
    // Twice the size, interpolated.
    let big = patch(&bar(2, 2, 4, 4), (8, 8), (16, 16), &SDR).unwrap();
    let opaque = |patch: &Patch| {
        let mut found = (usize::MAX, usize::MAX, 0, 0);
        for (index, pixel) in patch.pixels.iter().enumerate() {
            let (x, y) = (patch.x + index % patch.width, patch.y + index / patch.width);
            if pixel[3] > 0.99 {
                found = (
                    found.0.min(x),
                    found.1.min(y),
                    found.2.max(x),
                    found.3.max(y),
                );
            }
        }
        found
    };
    // It covers 4 to 11; its edges are part covered.
    assert_eq!(opaque(&big), (5, 5, 10, 10));
    let edge = big.pixels[(8 - big.y) * big.width + (4 - big.x)];
    assert!((edge[3] - 0.75).abs() < 1e-3, "{edge:?}");
    // Half the size, averaged: its outline and middle mix to gray.
    let small = patch(&bar(2, 2, 4, 4), (8, 8), (4, 4), &SDR).unwrap();
    assert_eq!(opaque(&small), (1, 1, 2, 2));
    let inside = small.pixels[(1 - small.y) * small.width + (1 - small.x)];
    assert!((inside[0] - 0.25).abs() < 1e-3, "{inside:?}");
    // A quarter of the size: all of a corner's 4x4 pixels averaged,
    // 7 of the 16 outline.
    let quarter = patch(&bar(0, 0, 8, 8), (8, 8), (2, 2), &SDR).unwrap();
    assert!(
        (quarter.pixels[0][0] - 9.0 / 16.0).abs() < 1e-3,
        "{quarter:?}"
    );
    // Off the frame, nothing.
    assert!(patch(&bar(20, 2, 4, 4), (8, 8), (8, 8), &SDR).is_none());
}

#[test]
fn takes_the_frames_colors() {
    use ffi::AVColorSpace::*;
    use ffi::AVColorTransferCharacteristic::*;
    let colors = |space, transfer| Colors {
        primaries: None,
        transfer: Some(transfer),
        space: Some(space),
    };
    let bt2020 = Coding {
        kr: 0.2627,
        kb: 0.0593,
        transfer: Transfer::Sdr,
    };
    let hdr10 = colors(AVCOL_SPC_BT2020_NCL, AVCOL_TRC_SMPTE2084);
    assert_eq!(
        Coding::of(&hdr10, true),
        Coding {
            transfer: Transfer::Pq,
            ..bt2020
        }
    );
    let hlg = colors(AVCOL_SPC_BT2020_NCL, AVCOL_TRC_ARIB_STD_B67);
    assert_eq!(
        Coding::of(&hlg, true),
        Coding {
            transfer: Transfer::Hlg,
            ..bt2020
        }
    );
    let sd = colors(AVCOL_SPC_SMPTE170M, AVCOL_TRC_BT709);
    assert_eq!(Coding::of(&sd, true).kr, 0.299);
    // 8-bit encodings are BT.709 SDR, whatever the source was.
    assert_eq!(Coding::of(&hdr10, false), SDR);
    assert_eq!(Coding::of(&Colors::default(), true), SDR);
}

#[test]
fn blends_into_nv12_in_limited_range() {
    let (width, height) = (8, 4);
    let mut luma = vec![100u8; width * height];
    let mut chroma = vec![128u8; width * height / 2];
    let patch = Patch {
        x: 2,
        y: 0,
        width: 2,
        height: 2,
        pixels: vec![
            [1.0, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.25, 0.5],
            [0.0, 0.0, 0.0, 1.0],
            [0.0; 4],
        ],
    };
    blend_luma(&mut luma, width, &patch, false);
    blend_chroma(&mut chroma, width, &patch, false);
    assert_eq!(&luma[..width], &[100, 100, 235, 168, 100, 100, 100, 100]);
    assert_eq!(
        &luma[width..2 * width],
        &[100, 100, 16, 100, 100, 100, 100, 100]
    );
    assert_eq!(&luma[2 * width..], &[100; 16]);
    // The chroma pair over x 2-3, y 0-1: 5/8 covered, red a little.
    assert_eq!(&chroma[2..4], &[128, 142]);
    assert_eq!(&chroma[..2], &[128, 128]);
}

#[test]
fn blends_into_p010_in_its_top_ten_bits() {
    let mut luma = vec![0u8; 8];
    let mut chroma = vec![0u8; 8];
    for plane in [&mut luma, &mut chroma] {
        for sample in plane.chunks_mut(2) {
            sample.copy_from_slice(&(512u16 << 6).to_le_bytes());
        }
    }
    let patch = Patch {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
        pixels: vec![[1.0, 0.0, 0.0, 1.0]; 4],
    };
    blend_luma(&mut luma, 4, &patch, true);
    blend_chroma(&mut chroma, 4, &patch, true);
    assert_eq!(u16::from_le_bytes([luma[0], luma[1]]), 940 << 6);
    assert_eq!(u16::from_le_bytes([luma[6], luma[7]]), 940 << 6);
    assert_eq!(u16::from_le_bytes([chroma[0], chroma[1]]), 512 << 6);
    assert_eq!(u16::from_le_bytes([chroma[4], chroma[5]]), 512 << 6);
}
