use super::*;

const NV12: Ycc = Ycc {
    kr: 0.299,
    kb: 0.114,
    bits: 8,
    full_range: true,
    shift: 0,
};

const X420: Ycc = Ycc {
    kr: 0.2627,
    kb: 0.0593,
    bits: 10,
    full_range: false,
    shift: 6,
};

#[test]
fn opaque_white_covers_luma_and_chroma() {
    // 4 x 2 gray; white over the left half, wholly.
    let mut overlay = Overlay::new(4, 2, None);
    for y in 0..2 {
        for x in 0..2 {
            overlay.blend(x, y, [1.0; 3], 1.0);
        }
    }
    let mut luma = vec![100u8; 8];
    let mut chroma = vec![200u8, 60, 200, 60];
    overlay.over_ycbcr(&mut luma, 4, &mut chroma, 4, NV12);
    assert_eq!(luma, [255, 255, 100, 100, 255, 255, 100, 100]);
    // White has no color; the right half keeps its own.
    assert_eq!(chroma, [128, 128, 200, 60]);
}

#[test]
fn chroma_takes_the_average_of_its_pixels() {
    // One of four pixels opaque white: a quarter of the way to no color.
    let mut overlay = Overlay::new(2, 2, None);
    overlay.blend(0, 0, [1.0; 3], 1.0);
    let mut luma = vec![0u8; 4];
    let mut chroma = vec![228u8, 28];
    overlay.over_ycbcr(&mut luma, 2, &mut chroma, 2, NV12);
    assert_eq!(luma, [255, 0, 0, 0]);
    assert_eq!(chroma, [203, 53]);
}

#[test]
fn ten_bit_samples_sit_high_in_video_range() {
    let mut overlay = Overlay::new(2, 2, None);
    for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        overlay.blend(x, y, [1.0; 3], 1.0);
    }
    let mut luma = vec![64u16 << 6; 4];
    let mut chroma = vec![512u16 << 6; 2];
    overlay.over_ycbcr(&mut luma, 2, &mut chroma, 2, X420);
    assert!(luma.iter().all(|&sample| sample == 940 << 6));
    assert!(chroma.iter().all(|&sample| sample == 512 << 6));
}

#[test]
fn half_covered_bgra_mixes() {
    let mut overlay = Overlay::new(1, 1, None);
    overlay.blend(0, 0, [1.0, 0.0, 0.0], 0.5);
    let mut pixels = vec![0u8, 0, 0, 255];
    overlay.over_bgra(&mut pixels, 4);
    assert_eq!(pixels, [0, 0, 128, 255]);
}

#[test]
fn layers_add_up_as_drawn_one_over_another() {
    // Half black, then half white over it, on a dark gray.
    let mut overlay = Overlay::new(1, 1, None);
    overlay.blend(0, 0, [0.0; 3], 0.5);
    overlay.blend(0, 0, [1.0; 3], 0.5);
    let mut pixels = vec![66u8, 66, 66, 255];
    overlay.over_bgra(&mut pixels, 4);
    // 66 halved, then halfway to white: 33 / 2 + 127.5.
    assert_eq!(pixels[..3], [144, 144, 144]);
}

#[test]
fn hdr_subtitles_are_at_reference_white() {
    let overlay = Overlay::new(1, 1, Some(Transfer::Pq));
    let white = overlay.signal([1.0; 3]);
    assert!(white.iter().all(|&s| (s - 0.5806).abs() < 0.002));
}
