use super::*;
use crate::media::hdr::{nits_to_pq, pq_to_nits};
use ::hdr::testing::reshaping;

/// 10-bit BT.2020 in video range, as Dolby Vision surfaces are.
const YCC: Ycc = Ycc {
    kr: 0.2627,
    kb: 0.0593,
    bits: 10,
    full_range: false,
    shift: 6,
};

/// A 10-bit sample, high in 16 bits, of `value` over 1.
fn sample(value: f32) -> u16 {
    ((value * 1024.0).round() as u16) << 6
}

/// The PQ R'G'B' a pixel's samples reshape to, worked out exactly.
fn expected(reshaping: &Reshaping, samples: [f32; 3]) -> [f32; 3] {
    let light = reshaping.lms_to_rgb(reshaping.lms_signal(samples).map(pq_to_nits));
    light.map(nits_to_pq)
}

/// The 10-bit code a 16-bit sample holds.
fn code(sample: u16) -> i32 {
    i32::from(sample >> 6)
}

#[test]
fn each_pixel_is_reshaped_as_the_shader_reshapes_it() {
    let reshaping = reshaping();
    // 4 × 4, each luma sample its own, one chroma all over.
    let chroma_values = [0.45, 0.55];
    let mut luma: Vec<u16> = (0..16).map(|at| sample(0.1 + at as f32 * 0.05)).collect();
    let mut chroma = [chroma_values.map(sample); 4].concat();
    let [cb, cr] = chroma_values.map(|value| f32::from(sample(value)) / 65536.0);
    let samples = |at: usize| [f32::from(luma[at]) / 65536.0, cb, cr];
    let rgb: Vec<[f32; 3]> = (0..16)
        .map(|at| expected(&reshaping, samples(at)))
        .collect();
    reshape(&reshaping, (&mut luma, 4), (&mut chroma, 4), 4, YCC);
    for (at, rgb) in rgb.iter().enumerate() {
        let want: u16 = YCC.write(YCC.luma(*rgb), YCC.luma_range());
        assert!(code(luma[at]).abs_diff(code(want)) <= 1, "luma at {at}");
    }
    // Each chroma pair, of the average of the four pixels it covers.
    for (at, pair) in chroma.as_chunks::<2>().0.iter().enumerate() {
        let (x, y) = (at % 2 * 2, at / 2 * 2);
        let covered = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)];
        let sum = covered.iter().fold([0.0; 3], |sum, &(x, y)| {
            let rgb = rgb[y * 4 + x];
            std::array::from_fn(|c| sum[c] + rgb[c] / 4.0)
        });
        let (cb, cr) = YCC.chroma(sum);
        for (sample, value) in pair.iter().zip([cb, cr]) {
            let want: u16 = YCC.write(value, YCC.chroma_range());
            assert!(code(*sample).abs_diff(code(want)) <= 1, "chroma at {at}");
        }
    }
}

#[test]
fn chroma_is_read_between_its_samples_as_a_gpus_sampler_reads_it() {
    let reshaping = reshaping();
    // 4 × 4 of one luma, under 2 × 2 chroma pairs each their own.
    let pairs = [[0.3, 0.6], [0.6, 0.35], [0.45, 0.7], [0.7, 0.4]];
    let mut luma = vec![sample(0.5); 16];
    let mut chroma: Vec<u16> = pairs.iter().flatten().map(|&value| sample(value)).collect();
    let pairs = pairs.map(|pair| pair.map(|value| f32::from(sample(value)) / 65536.0));
    reshape(&reshaping, (&mut luma, 4), (&mut chroma, 4), 4, YCC);
    // Pixel centers fall a quarter of a sample in from their own, so each
    // takes a quarter of its neighbor's, if it has one that way.
    let toward_the_second = [0.0, 0.25, 0.75, 1.0];
    let mut told = 0;
    for y in 0..4 {
        for x in 0..4 {
            let (along, down) = (toward_the_second[x], toward_the_second[y]);
            let mix = |a: [f32; 2], b: [f32; 2], t: f32| [0, 1].map(|c| a[c] + (b[c] - a[c]) * t);
            let [cb, cr] = mix(
                mix(pairs[0], pairs[1], along),
                mix(pairs[2], pairs[3], along),
                down,
            );
            let luma_of = |cb: f32, cr: f32| -> u16 {
                let rgb = expected(&reshaping, [0.5, cb, cr]);
                YCC.write(YCC.luma(rgb), YCC.luma_range())
            };
            let want = luma_of(cb, cr);
            assert!(
                code(luma[y * 4 + x]).abs_diff(code(want)) <= 1,
                "at {x}, {y}"
            );
            let [cb, cr] = pairs[y / 2 * 2 + x / 2];
            if code(luma_of(cb, cr)).abs_diff(code(want)) > 2 {
                told += 1;
            }
        }
    }
    assert!(told >= 8, "read as the nearest pair, most would differ");
}

#[test]
fn planes_too_narrow_for_their_pixels_are_left_alone() {
    let mut luma = vec![sample(0.5); 16];
    // Rows of one sample, not the pair two pixels' chroma needs.
    let mut chroma = vec![sample(0.5); 2];
    reshape(&reshaping(), (&mut luma, 4), (&mut chroma, 1), 4, YCC);
    assert!(luma.iter().all(|&own| own == sample(0.5)));
    assert!(chroma.iter().all(|&own| own == sample(0.5)));
}
