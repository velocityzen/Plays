use super::*;
use crate::media::tests::made::media;

const SLICE: u8 = 1;
const VPS: u8 = 32;
const SPS: u8 = 33;
const PPS: u8 = 34;
const SEI: u8 = 39;
const RPU: u8 = 62;
const ENHANCEMENT: u8 = 63;

/// A NAL unit of `kind` in `layer`, holding `body`.
pub(super) fn unit(kind: u8, layer: u8, body: &[u8]) -> Vec<u8> {
    let mut unit = vec![kind << 1 | layer >> 5, (layer & 0x1f) << 3 | 1];
    unit.extend(body);
    unit
}

/// An `hvcC` of four-byte sizes with `arrays`: each a kind of NAL unit and
/// the units.
fn record(arrays: &[(u8, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut record = vec![1, 0x22, 0x20, 0, 0, 0, 0xb0, 0, 0, 0, 0, 0, 0x96, 0xf0, 0];
    record.extend([0xfc, 0xfd, 0xfa, 0xfa, 0, 0, 0x0f]);
    record.push(u8::try_from(arrays.len()).unwrap());
    for (kind, units) in arrays {
        record.push(0x80 | kind);
        record.extend(u16::try_from(units.len()).unwrap().to_be_bytes());
        for unit in units {
            record.extend(u16::try_from(unit.len()).unwrap().to_be_bytes());
            record.extend(unit);
        }
    }
    record
}

/// `units`, each after its size in `bytes` bytes.
pub(super) fn sized(units: &[Vec<u8>], bytes: usize) -> Vec<u8> {
    units
        .iter()
        .flat_map(|unit| {
            let size = u32::try_from(unit.len()).unwrap().to_be_bytes();
            size[4 - bytes..]
                .iter()
                .chain(unit)
                .copied()
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `units`, each after a four-byte start code.
pub(super) fn start_coded(units: &[Vec<u8>]) -> Vec<u8> {
    units
        .iter()
        .flat_map(|unit| [0, 0, 0, 1].iter().chain(unit).copied().collect::<Vec<_>>())
        .collect()
}

/// Ten frames of HEVC with alpha, as VideoToolbox makes it: a second
/// layer, its parameter sets in the `hvcC` and its slices in every packet.
pub(in crate::media::decoder) fn two_layer_media() -> std::path::PathBuf {
    media(
        "ten-frames-hevc-alpha.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=10:duration=1,format=bgra",
            "-c:v",
            "hevc_videotoolbox",
            "-alpha_quality",
            "0.75",
            "-tag:v",
            "hvc1",
        ],
    )
}

#[test]
fn a_layer_is_read_from_its_units_header() {
    for layer in [0, 1, 31, 32, 63] {
        assert_eq!(super::layer(&unit(SPS, layer, &[7])), layer);
    }
    assert_eq!(super::layer(&[0x42]), 0, "too short for one");
    assert_eq!(super::layer(&[]), 0);
}

#[test]
fn units_are_sized_as_an_hvcc_says_or_follow_start_codes() {
    let mut four = record(&[]);
    assert_eq!(Framing::of(&four), Framing::Sized(4));
    four[21] = 0xfd;
    assert_eq!(Framing::of(&four), Framing::Sized(2));
    four.truncate(10);
    assert_eq!(Framing::of(&four), Framing::Sized(4), "as FFmpeg reads it");
    let annex_b = start_coded(&[unit(VPS, 0, &[1, 2])]);
    assert_eq!(Framing::of(&annex_b), Framing::StartCodes);
    assert_eq!(Framing::of(&annex_b[1..]), Framing::StartCodes);
    assert_eq!(Framing::of(&[]), Framing::StartCodes);
}

#[test]
fn an_hvcc_keeps_the_base_layers_units_alone() {
    let base = [
        (VPS, vec![unit(VPS, 0, &[1, 2, 3])]),
        (SPS, vec![unit(SPS, 0, &[4, 5])]),
        (PPS, vec![unit(PPS, 0, &[6])]),
        (SEI, vec![unit(SEI, 0, &[7, 8])]),
    ];
    // An lhvC's arrays after the hvcC's, as FFmpeg's MP4 demuxer adds them.
    let mut both = base.to_vec();
    both.push((SPS, vec![unit(SPS, 1, &[9])]));
    both.push((PPS, vec![unit(PPS, 1, &[10, 11])]));
    assert_eq!(base_record(&record(&both)), Some(record(&base)));

    // And in the same arrays, as an encoder of HEVC with alpha puts them.
    let mixed = [
        (VPS, vec![unit(VPS, 0, &[1])]),
        (SPS, vec![unit(SPS, 0, &[2]), unit(SPS, 1, &[3])]),
        (PPS, vec![unit(PPS, 1, &[4]), unit(PPS, 0, &[5])]),
    ];
    let kept = [
        (VPS, vec![unit(VPS, 0, &[1])]),
        (SPS, vec![unit(SPS, 0, &[2])]),
        (PPS, vec![unit(PPS, 0, &[5])]),
    ];
    assert_eq!(base_record(&record(&mixed)), Some(record(&kept)));

    // What follows the arrays stays.
    let mut after = record(&both);
    after.extend([0xaa, 0xbb]);
    let mut kept = record(&base);
    kept.extend([0xaa, 0xbb]);
    assert_eq!(base_record(&after), Some(kept));
}

#[test]
fn an_hvcc_of_one_layer_or_cut_short_is_left_as_it_is() {
    let one = record(&[
        (VPS, vec![unit(VPS, 0, &[1])]),
        (SPS, vec![unit(SPS, 0, &[2]), unit(SPS, 0, &[3])]),
    ]);
    assert_eq!(base_record(&one), None);
    assert_eq!(base_record(&record(&[])), None);

    let two = record(&[
        (SPS, vec![unit(SPS, 0, &[2])]),
        (SPS, vec![unit(SPS, 1, &[3, 4, 5])]),
    ]);
    for end in 0..two.len() {
        assert_eq!(base_record(&two[..end]), None, "cut at {end}");
    }
}

#[test]
fn packets_keep_the_base_layers_units_alone() {
    let units = [
        unit(SEI, 1, &[1]),
        unit(SLICE, 0, &[2, 3, 4]),
        unit(SLICE, 1, &[5, 6]),
        unit(RPU, 0, &[7, 8]),
        unit(ENHANCEMENT, 0, &[9]),
        unit(SLICE, 2, &[10]),
    ];
    let kept = [
        unit(SLICE, 0, &[2, 3, 4]),
        unit(RPU, 0, &[7, 8]),
        unit(ENHANCEMENT, 0, &[9]),
    ];
    for bytes in 1..=4 {
        assert_eq!(
            base_units(&sized(&units, bytes), Framing::Sized(bytes)),
            Some(sized(&kept, bytes)),
            "sizes of {bytes} bytes"
        );
    }
    assert_eq!(
        base_units(&start_coded(&units), Framing::StartCodes),
        Some(start_coded(&kept))
    );
    let other = [unit(SLICE, 1, &[1]), unit(SEI, 1, &[2])];
    assert_eq!(
        base_units(&sized(&other, 4), Framing::Sized(4)),
        Some(Vec::new()),
        "none of the base layer's"
    );
    // Three-byte start codes, and zeros after a unit, as Annex B allows.
    let mut short = vec![0, 0, 1];
    short.extend(&units[0]);
    short.extend([0, 0, 0, 0, 1]);
    short.extend(&units[1]);
    short.extend([0, 0, 1]);
    short.extend(&units[3]);
    assert_eq!(
        base_units(&short, Framing::StartCodes),
        Some(start_coded(&[units[1].clone(), units[3].clone()]))
    );
}

#[test]
fn a_packet_of_the_base_layer_alone_or_unreadable_is_left_as_it_is() {
    let base = [unit(SLICE, 0, &[1, 2]), unit(RPU, 0, &[3])];
    assert_eq!(base_units(&sized(&base, 4), Framing::Sized(4)), None);
    assert_eq!(base_units(&start_coded(&base), Framing::StartCodes), None);
    assert_eq!(base_units(&[], Framing::Sized(4)), None);

    let both = sized(&[unit(SLICE, 0, &[1]), unit(SLICE, 1, &[2])], 4);
    for end in 0..both.len() {
        assert_eq!(
            base_units(&both[..end], Framing::Sized(4)),
            None,
            "cut at {end}"
        );
    }
}

#[test]
fn hevc_of_one_layer_and_other_codecs_are_decoded_as_they_are() {
    let path = media(
        "ten-frames-hevc.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=10:duration=1",
            "-c:v",
            "libx265",
            "-x265-params",
            "log-level=error",
        ],
    );
    let input = ffmpeg_safe::input_file(&path).unwrap();
    let parameters = input.stream(0).unwrap().parameters();
    assert_eq!(parameters.id(), codec::Id::HEVC);
    assert!(BaseLayer::of(&parameters).is_none());

    let mut parameters = codec::Parameters::new();
    let mut extradata = record(&[(SPS, vec![unit(SPS, 1, &[1])])]);
    // SAFETY: the parameters are freshly allocated.
    unsafe { (*parameters.as_mut_ptr()).codec_id = codec::Id::H264.into() };
    set_extradata(&mut parameters, &extradata).unwrap();
    assert!(BaseLayer::of(&parameters).is_none(), "not HEVC");
    // SAFETY: as above.
    unsafe { (*parameters.as_mut_ptr()).codec_id = codec::Id::HEVC.into() };
    assert!(BaseLayer::of(&parameters).is_some());
    extradata.pop();
    set_extradata(&mut parameters, &extradata).unwrap();
    assert!(BaseLayer::of(&parameters).is_none(), "cut short");
}

#[test]
fn hevc_of_two_layers_is_given_its_base_layer_alone() {
    let mut input = ffmpeg_safe::input_file(&two_layer_media()).unwrap();
    let parameters = input.stream(0).unwrap().parameters();
    let layers = |data: &[u8], framing| {
        let mut layers: Vec<u8> = units(data, framing)
            .unwrap()
            .into_iter()
            .map(layer)
            .collect();
        layers.dedup();
        layers
    };
    let (base, kept) = BaseLayer::of(&parameters).unwrap();
    assert_eq!(base.framing, Framing::Sized(4));
    let all_units = |record: &[u8]| -> Vec<Vec<u8>> {
        let mut rest = &record[23..];
        let mut all = Vec::new();
        while let Some((&[_, high, low], after)) = rest.split_first_chunk::<3>() {
            rest = after;
            for _ in 0..u16::from_be_bytes([high, low]) {
                let (&size, after) = rest.split_first_chunk::<2>().unwrap();
                let (unit, after) = after.split_at(usize::from(u16::from_be_bytes(size)));
                all.push(unit.to_vec());
                rest = after;
            }
        }
        all
    };
    let before = all_units(ffmpeg_safe::extradata(&parameters));
    let after = all_units(ffmpeg_safe::extradata(&kept));
    assert!(before.iter().any(|unit| layer(unit) == 1), "a second layer");
    let base_units: Vec<_> = before.into_iter().filter(|unit| layer(unit) == 0).collect();
    assert_eq!(after, base_units);

    let mut count = 0;
    for (_, packet) in input.packets() {
        let data = packet.data().unwrap();
        assert!(layers(data, base.framing).contains(&1), "in every packet");
        let kept = base.packet(&packet).unwrap();
        assert_eq!(layers(kept.data().unwrap(), base.framing), [0]);
        assert_eq!(
            (
                kept.pts(),
                kept.dts(),
                kept.duration(),
                kept.flags(),
                kept.stream()
            ),
            (
                packet.pts(),
                packet.dts(),
                packet.duration(),
                packet.flags(),
                packet.stream()
            )
        );
        count += 1;
    }
    assert_eq!(count, 10);
}
