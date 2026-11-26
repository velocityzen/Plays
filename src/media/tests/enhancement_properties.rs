use hegel::TestCase;
use hegel::generators as gs;

use super::*;

fn int(tc: &TestCase, low: i64, high: i64) -> i64 {
    tc.draw(gs::integers::<i64>().min_value(low).max_value(high))
}

/// Dequantizing as an RPU might: 8 to 10 bits, coefficients up to 2 with
/// 13 to 32 bits of fraction.
fn nlq(tc: &TestCase) -> (Nlq, u32, u32) {
    let el_depth = int(tc, 8, 10) as u32;
    let denom = int(tc, 13, 32) as u32;
    let most = 2i64 << denom;
    let nlq = Nlq {
        offset: int(tc, 0, (1 << el_depth) - 1),
        in_max: int(tc, 0, most),
        slope: int(tc, 0, most),
        threshold: int(tc, 0, most),
    };
    (nlq, el_depth, denom)
}

#[hegel::test]
fn the_residual_grows_with_the_code_both_ways_alike(tc: TestCase) {
    let (nlq, el_depth, denom) = nlq(&tc);
    let d = int(&tc, 0, 1 << el_depth);
    let (up, down) = (
        dequantize(nlq.offset + d, nlq, el_depth, denom),
        dequantize(nlq.offset - d, nlq, el_depth, denom),
    );
    // Rounded down, so a negative one is at most one further.
    assert!(up + down == 0 || up + down == -1, "{up} and {down}");
    assert!(up >= 0);
    assert!(dequantize(nlq.offset + d + 1, nlq, el_depth, denom) >= up);
    // At most vdr_in_max, in 16 bits.
    let most = (i128::from(nlq.in_max) << 16 >> denom) as i32;
    assert!(up <= most && down >= -most - 1, "{up}, {down}, past {most}");
}

#[hegel::test]
fn the_master_rounds_to_the_nearest_ten_bit_code(tc: TestCase) {
    let mapped = int(&tc, 0, 0xffff) as u16;
    let residual = int(&tc, -(1 << 17), 1 << 17) as i32;
    let exact = (f64::from(mapped) + f64::from(residual)) / 64.0;
    let nearest = (exact + 0.5).floor().clamp(0.0, 1023.0) as u16;
    assert_eq!(reconstruct(mapped, residual), nearest);
}

#[hegel::test]
fn a_flat_enhancement_layer_stays_flat(tc: TestCase) {
    let code = int(&tc, 0, 1023) as u16;
    let (width, height) = (int(&tc, 1, 9) as usize, int(&tc, 1, 9) as usize);
    let plane = Codes {
        codes: vec![code; width * height],
        width,
        height,
    };
    let chroma = tc.draw(gs::booleans());
    for y in 0..2 * height {
        assert_eq!(
            enhancement_row(&plane, chroma, y, 2 * width, true),
            vec![code; 2 * width]
        );
    }
}

#[hegel::test]
fn mapped_as_it_is_the_base_layer_comes_back(tc: TestCase) {
    let bl_depth = int(&tc, 8, 10) as u32;
    let s = int(&tc, 0, (1 << bl_depth) - 1);
    let mapped = polynomial(s, 1, &[0, 1 << 23, 0], bl_depth, 23);
    assert_eq!(i64::from(mapped), s << (16 - bl_depth));
    assert_eq!(i64::from(reconstruct(mapped, 0)), s << (10 - bl_depth));
}
