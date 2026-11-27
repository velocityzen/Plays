use ffmpeg_next::util::color::{Space, TransferCharacteristic as Trc};
use ffmpeg_next::util::format::Pixel;

use super::*;

const METAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/gpui/src/platform/mac/shaders.metal"
));
const WGSL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/gpui/src/platform/blade/shaders.wgsl"
));

#[test]
fn reads_hdr_by_its_transfer_in_any_matrix() {
    let of = |transfer, space| {
        let mut frame = frame::Video::new(Pixel::YUV420P10LE, 2, 2);
        frame.set_color_transfer_characteristic(transfer);
        frame.set_color_space(space);
        Transfer::of(&frame)
    };
    assert_eq!(of(Trc::SMPTE2084, Space::BT2020NCL), Some(Transfer::Pq));
    assert_eq!(of(Trc::ARIB_STD_B67, Space::BT2020NCL), Some(Transfer::Hlg));
    assert_eq!(of(Trc::SMPTE2084, Space::Unspecified), Some(Transfer::Pq));
    assert_eq!(of(Trc::SMPTE2084, Space::IPT_C2), Some(Transfer::Pq));
    assert_eq!(of(Trc::ARIB_STD_B67, Space::SMPTE170M), Some(Transfer::Hlg));
    assert_eq!(of(Trc::BT709, Space::BT2020NCL), None);
    assert_eq!(of(Trc::BT2020_10, Space::BT2020NCL), None);
    assert_eq!(of(Trc::Unspecified, Space::Unspecified), None);
}

/// The part of `shader` from `head` to the end of its function.
fn body<'a>(shader: &'a str, head: &str) -> &'a str {
    let start = shader.find(head).unwrap_or_else(|| panic!("no {head}"));
    let rest = &shader[start..];
    &rest[..rest.find("\n}").unwrap_or(rest.len())]
}

/// The number `name` is set to in `code` (`name = …;`, typed or not):
/// numbers divided and multiplied in turn.
fn value(code: &str, name: &str) -> f32 {
    let expression = code
        .lines()
        .find_map(|line| {
            let (left, right) = line.split_once(" = ")?;
            left.split_whitespace()
                .any(|word| word.trim_end_matches(':') == name)
                .then(|| right.trim().trim_end_matches(';'))
        })
        .unwrap_or_else(|| panic!("no {name}"));
    let mut words = expression.split_whitespace();
    let number = |word: Option<&str>| -> f32 {
        word.and_then(|word| word.parse().ok())
            .unwrap_or_else(|| panic!("{name} = {expression}"))
    };
    let mut value = number(words.next());
    while let Some(op) = words.next() {
        match op {
            "/" => value /= number(words.next()),
            "*" => value *= number(words.next()),
            _ => panic!("{name} = {expression}"),
        }
    }
    value
}

/// The numbers of the vector `code` dots `with` with.
fn dotted(code: &str, with: &str) -> Vec<f32> {
    let at = code.find(with).unwrap_or_else(|| panic!("no {with}"));
    let rest = &code[at + with.len()..];
    let vector = &rest[rest.find('(').unwrap() + 1..rest.find(')').unwrap()];
    vector
        .split(',')
        .map(|number| number.trim().parse().unwrap())
        .collect()
}

/// The factor `code` returns times what follows.
fn returned_factor(code: &str) -> f32 {
    let rest = &code[code.find("return ").expect("a return") + "return ".len()..];
    rest.split_whitespace().next().unwrap().parse().unwrap()
}

#[test]
fn the_shaders_have_the_same_constants() {
    // Each shader: its prefix for its own constants, and where it decodes
    // PQ and HLG.
    for (shader, prefix, pq, hlg) in [
        (METAL, "", "float3 pq_to_nits(", "float3 hlg_to_nits("),
        (WGSL, "YUV_", "fn yuv_pq_to_nits(", "fn yuv_hlg_to_nits("),
    ] {
        let own = |name: &str| value(shader, &format!("{prefix}{name}"));
        assert_eq!(own("REFERENCE_WHITE_NITS"), REFERENCE_WHITE_NITS);
        assert_eq!(own("HLG_PEAK_NITS"), HLG_PEAK_NITS);
        assert_eq!(own("HLG_GAMMA"), HLG_GAMMA);
        assert_eq!(own("TONE_KNEE"), KNEE);

        let pq = body(shader, pq);
        let coefficients = [("m1", PQ_M1), ("m2", PQ_M2), ("c1", PQ_C1)];
        for (name, want) in coefficients
            .into_iter()
            .chain([("c2", PQ_C2), ("c3", PQ_C3)])
        {
            assert_eq!(value(pq, name), want, "{prefix}PQ's {name}");
        }
        assert_eq!(returned_factor(pq), PQ_PEAK_NITS);

        let hlg = body(shader, hlg);
        assert_eq!(value(hlg, "a"), HLG_A, "{prefix}HLG's a");
        assert_eq!(value(hlg, "c"), HLG_C, "{prefix}HLG's c");
        assert_eq!(dotted(hlg, "dot(scene, "), BT2020_LUMA, "{prefix}luma");
    }
}
