use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::tests::{sized, start_coded, unit};
use super::*;

/// NAL units of any kind and layer. Their bodies have no zeros where
/// they're to follow start codes, as escaped units have no start code in
/// them, nor end in a zero.
#[hegel::composite]
fn nal_units(tc: &TestCase, start_codes: bool) -> Vec<Vec<u8>> {
    let byte = gs::integers::<u8>().min_value(u8::from(start_codes));
    let one = hegel::tuples!(
        gs::integers::<u8>().max_value(63),
        gs::integers::<u8>().max_value(63),
        gs::vecs(byte).max_size(40)
    )
    .map(|(kind, layer, body)| unit(kind, layer, &body));
    tc.draw(gs::vecs(one).max_size(8))
}

#[hegel::test]
fn a_packets_base_layer_is_its_units_of_layer_0_in_their_order(tc: TestCase) {
    let start_codes = tc.draw(gs::booleans());
    let units = tc.draw(nal_units(start_codes));
    let bytes = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let framing = if start_codes {
        Framing::StartCodes
    } else {
        Framing::Sized(bytes)
    };
    let frame = |units: &[Vec<u8>]| match framing {
        Framing::Sized(bytes) => sized(units, bytes),
        Framing::StartCodes => start_coded(units),
    };
    let kept: Vec<_> = units.iter().filter(|u| layer(u) == 0).cloned().collect();
    let want = (kept.len() < units.len()).then(|| frame(&kept));
    assert_eq!(base_units(&frame(&units), framing), want);
}

#[hegel::test]
fn whatever_it_reads_it_leaves_the_base_layer_alone(tc: TestCase) {
    let data = tc.draw(gs::binary().max_size(300));
    let bytes = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    for framing in [Framing::Sized(bytes), Framing::StartCodes] {
        if let Some(base) = base_units(&data, framing) {
            let units = units(&base, framing).unwrap();
            assert!(units.iter().all(|unit| layer(unit) == 0), "{units:?}");
            assert_eq!(base_units(&base, framing), None, "nothing more to take");
        }
    }
    if let Some(base) = base_record(&data) {
        assert_eq!(Framing::of(&base), Framing::of(&data));
        assert_eq!(base_record(&base), None, "nothing more to take");
    }
}
