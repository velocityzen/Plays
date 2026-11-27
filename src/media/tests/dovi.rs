use ::hdr::testing::{attach_rpu, profile5};
use ffmpeg::ffi;

use super::*;
use crate::media::hdr::{REFERENCE_WHITE_NITS, pq_to_nits};

/// A profile 5 frame of one color all over, its luma and chroma `codes`
/// (10-bit), with `profile5`'s RPU as side data, mastered on a display
/// whose peak is `source_max_pq`.
pub(crate) fn rpu_frame(size: (u32, u32), codes: [u16; 3], source_max_pq: u16) -> frame::Video {
    use ffmpeg::util::format::Pixel;

    let (header, mapping, mut color) = profile5();
    color.source_max_pq = source_max_pq;
    let mut frame = frame::Video::new(Pixel::P010LE, size.0, size.1);
    for (plane, codes) in [(0, &codes[..1]), (1, &codes[1..])] {
        let (data, stride) = ffmpeg_safe::plane_mut(&mut frame, plane).unwrap();
        for row in data.chunks_mut(stride) {
            for (x, sample) in row.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                *sample = (codes[x % codes.len()] << 6).to_le_bytes();
            }
        }
    }
    attach_rpu(&mut frame, &header, &mapping, &color);
    frame
}

/// `profile5`'s RPU as an encoder writes it, an HEVC NAL unit of type 62.
fn profile5_rpu_unit() -> Vec<u8> {
    use crate::media::enhancement::tests::{Bits, DENOM, rpu_unit};

    let mut bits = Bits::default();
    // The header: type 2, format 18, profile 0, level 0, then the sequence:
    // fixed point, normalized, full range, 10-bit layers, 12-bit master.
    bits.put(2, 6);
    bits.put(18, 11);
    bits.put(0, 4);
    bits.put(0, 4);
    for (value, width) in [(1, 1), (0, 1), (0, 2)] {
        bits.put(value, width);
    }
    bits.ue(u64::from(DENOM));
    bits.put(1, 2);
    bits.put(1, 1);
    for depth in [10, 10, 12] {
        bits.ue(depth - 8);
    }
    // No filters, no compression, no residual; metadata, all of it anew.
    for (value, width) in [(0, 1), (0, 3), (0, 1), (1, 1), (1, 1), (0, 1)] {
        bits.put(value, width);
    }
    // Mapping 0: luma's pivots at 0, 512 and 1023, chroma's at the ends,
    // each the last plus the next.
    for _ in 0..3 {
        bits.ue(0);
    }
    bits.ue(1);
    for pivot in [0, 512, 511] {
        bits.put(pivot, 10);
    }
    for _ in 0..2 {
        bits.ue(0);
        bits.put(0, 10);
        bits.put(1023, 10);
    }
    // One partition.
    bits.ue(0);
    bits.ue(0);
    // Polynomials: luma x, then x² / 2 + 1/4; each chroma a line.
    let pieces: [&[&[i64]]; 3] = [
        &[&[0, 1 << DENOM], &[1 << (DENOM - 2), 0, 1 << (DENOM - 1)]],
        &[&[-649_193, 10_699_938]],
        &[&[-649_193, 10_699_938]],
    ];
    for coefficients in pieces.iter().copied().flatten() {
        let order = coefficients.len() as u64 - 1;
        bits.ue(0);
        bits.ue(order - 1);
        if order == 1 {
            // Not interpolated.
            bits.put(0, 1);
        }
        for &coefficient in *coefficients {
            bits.coefficient(coefficient, true);
        }
    }
    // Display management: ids, no scene cut, IPTPQc2's matrices, PQ.
    for _ in 0..3 {
        bits.ue(0);
    }
    for value in [8192i16, 799, 1681, 8192, -933, 1091, 8192, 267, -5545] {
        bits.put(u64::from(value.cast_unsigned()), 16);
    }
    for offset in [0, 1 << 27, 1 << 27] {
        bits.put(offset, 32);
    }
    for value in [17081i16, -349, -349, -349, 17081, -349, -349, -349, 17081] {
        bits.put(u64::from(value.cast_unsigned()), 16);
    }
    for (value, width) in [
        (65535, 16),
        (0, 16),
        (0, 16),
        (0, 32),
        // 12 bits, full range.
        (12, 5),
        (0, 2),
        (0, 2),
        (1, 2),
        // 0.005 and 1,000 nits.
        (62, 12),
        (3079, 12),
        (42, 10),
    ] {
        bits.put(value, width);
    }
    // No extension blocks.
    bits.ue(0);
    rpu_unit(bits)
}

/// Ten frames of Dolby Vision profile 20, with one view: 10-bit HEVC whose
/// codes are IPT, tagged IPT-C2, PQ and BT.2020 in full range, as such a
/// file is, each with `profile5`'s RPU. Its left half is
/// `renders_a_known_frame`'s skin tone, its right half its green.
fn profile_20_media() -> std::path::PathBuf {
    use crate::media::tests::made::{kept, made_in, media};

    let ipt = media(
        "ten-frames-ipt.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "color=size=320x180:rate=10:duration=1",
            "-vf",
            "format=yuv420p10le,geq=lum='if(lt(X,W/2),432,387)':cb='if(lt(X,W/2),481,365)':cr='if(lt(X,W/2),492,588)'",
            "-c:v",
            "libx265",
            "-x265-params",
            "log-level=error:bframes=0:lossless=1",
            "-bsf:v",
            "hevc_metadata=colour_primaries=9:transfer_characteristics=16:matrix_coefficients=15:video_full_range_flag=1",
        ],
    );
    let recipe = std::fs::read_to_string(ipt.with_added_extension("made")).unwrap();
    made_in(
        &kept(),
        "profile-20.mp4",
        &format!("{recipe:?} as profile 20, with profile 5's RPU"),
        |out| as_profile_20(&ipt, out).map_err(|err| err.to_string()),
    )
}

/// `from`'s video as Dolby Vision profile 20, each frame with
/// `profile5_rpu_unit`, into `to`.
fn as_profile_20(from: &std::path::Path, to: &std::path::Path) -> Result<(), ffmpeg::Error> {
    use crate::media::tests::{appended, set_dovi_configuration, video_packets};

    let (parameters, from_base, packets) = video_packets(from)?;
    let rpu = profile5_rpu_unit();
    let mut output = ffmpeg::format::output(to)?;
    {
        let mut stream = output.add_stream(ffmpeg::encoder::find(ffmpeg::codec::Id::None))?;
        stream.set_parameters(parameters);
        stream.set_time_base(from_base);
        // SAFETY: the stream's parameters are valid.
        unsafe { (*(*stream.as_mut_ptr()).codecpar).codec_tag = 0 };
        // Profile 20, an RPU and a base layer nothing else shows.
        set_dovi_configuration(&mut stream, [1, 0, 20, 6, 1, 0, 1, 0, 0]);
    }
    // MP4 keeps a Dolby Vision configuration only so.
    // SAFETY: setting a field of a valid context.
    unsafe { (*output.as_mut_ptr()).strict_std_compliance = ffi::FF_COMPLIANCE_UNOFFICIAL };
    output.write_header()?;
    let to_base = output.stream(0).unwrap().time_base();
    for packet in &packets {
        let mut packet = appended(packet, std::slice::from_ref(&rpu));
        packet.set_stream(0);
        packet.rescale_ts(from_base, to_base);
        packet.set_position(-1);
        packet.write_interleaved(&mut output)?;
    }
    output.write_trailer()
}

/// Profile 20's pictures are IPT, though tagged PQ: played, decoded on the
/// GPU or in software, they show as profile 5's do.
#[test]
fn plays_profile_20_as_ipt() {
    use crate::media::{still, tests::first_frame};

    let path = profile_20_media();
    let input = ffmpeg_safe::input_file(&path).unwrap();
    let configuration = ffmpeg_safe::DolbyVision::of(&input.stream(0).unwrap().parameters());
    assert_eq!(configuration.map(|c| c.profile), Some(20));
    drop(input);
    for hardware in [true, false] {
        let shown = still::shown(&first_frame(&path, hardware), false).unwrap();
        let (width, height) = (shown.width as usize, shown.height as usize);
        for (x, want) in [(width / 4, [148u8, 123, 107]), (width * 3 / 4, [4, 136, 1])] {
            let got = shown.pixels[height / 2 * width + x].map(|c| (c / 257) as u8);
            assert!(
                want.iter().zip(got).all(|(&a, b)| a.abs_diff(b) <= 2),
                "hardware {hardware}, at {x}: want {want:?}, got {got:?}"
            );
        }
    }
}

#[test]
fn reshapes_ipt_however_its_tagged() {
    use ffmpeg::util::color::{Primaries, Range, Space, TransferCharacteristic};

    let tagged = |space, transfer| {
        let mut frame = rpu_frame((4, 2), [512; 3], 3079);
        frame.set_color_range(Range::JPEG);
        frame.set_color_space(space);
        frame.set_color_primaries(Primaries::BT2020);
        frame.set_color_transfer_characteristic(transfer);
        reshaping(&frame).is_some()
    };
    // Profile 5, untagged.
    assert!(reshaping(&rpu_frame((4, 2), [512; 3], 3079)).is_some());
    // Profile 20: IPT-C2, though PQ.
    assert!(tagged(Space::IPT_C2, TransferCharacteristic::SMPTE2084));
    // Profile 8: HDR10 or HLG, shown as it is.
    for transfer in [
        TransferCharacteristic::SMPTE2084,
        TransferCharacteristic::ARIB_STD_B67,
    ] {
        assert!(!tagged(Space::BT2020NCL, transfer), "{transfer:?}");
    }
}

#[test]
fn renders_planes_through_the_tone_mapper() {
    let reshaping = ::hdr::testing::reshaping();
    let mapper = ToneMapper::new(Transfer::Pq);
    // One pixel, neutral, that reshapes to PQ's reference white (in the
    // second piece of the luma curve, x² / 2 + 1/4).
    let neutral = ::hdr::testing::NEUTRAL;
    let white = 0.5806f32;
    let plane = |v: f32| ((v * 65536.0).round() as u16).to_le_bytes().to_vec();
    let (y, c) = (plane(((white - 0.25) * 2.0).sqrt()), plane(neutral));
    let out = render(&reshaping, &mapper, [(&y, 2), (&c, 2), (&c, 2)], 1, 1);
    let [b, g, r, a] = out[..4] else {
        unreachable!()
    };
    assert_eq!(a, 255);
    assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1, "{out:?}");
    // Reference white comes out just under SDR white, as for HDR10.
    assert!((235..=250).contains(&g), "{out:?}");
    assert!((pq_to_nits(white) - REFERENCE_WHITE_NITS).abs() < 1.0);
}

/// Render 10-bit base layer codes (I, P, T per pixel, row by row) as the
/// decoder hands them over: in the top bits of 16-bit planes whose rows
/// are padded with junk. Returns RGB per pixel.
fn render_codes(codes: &[[u16; 3]], width: usize) -> Vec<[u8; 3]> {
    let height = codes.len() / width;
    let stride = width * 2 + 6;
    let planes: [Vec<u8>; 3] = std::array::from_fn(|c| {
        let mut plane = vec![0xff; stride * height];
        for (i, pixel) in codes.iter().enumerate() {
            let at = (i / width) * stride + (i % width) * 2;
            plane[at..at + 2].copy_from_slice(&(pixel[c] << 6).to_le_bytes());
        }
        plane
    });
    let mapper = ToneMapper::new(Transfer::Pq);
    let planes = [0, 1, 2].map(|c| (planes[c].as_slice(), stride));
    let out = render(&::hdr::testing::reshaping(), &mapper, planes, width, height);
    let (pixels, _) = out.as_chunks::<4>();
    assert!(pixels.iter().all(|&[.., a]| a == 255));
    pixels.iter().map(|&[b, g, r, _]| [r, g, b]).collect()
}

#[test]
fn renders_a_known_frame() {
    // Colors put into IPTPQc2 and through this frame's curves backwards
    // to 10-bit codes, the way an encoder makes a profile 5 picture. The
    // bytes they should come back as were worked out apart from this
    // code, from the standards (ST 2084, the IPTPQc2 matrices, BT.2087,
    // sRGB) and the tone curve, for the codes as rounded; where the
    // rounding moves a color, it's by a step or two.
    let frame: [([u16; 3], [u8; 3]); 8] = [
        // Black.
        ([0, 464, 464], [0, 0, 0]),
        // Reference white, just under SDR white as for HDR10.
        ([833, 464, 464], [245, 244, 243]),
        // Grey at 20 nits.
        ([366, 464, 464], [89, 89, 88]),
        // A skin tone, BT.709 (60, 40, 30) nits.
        ([432, 481, 492], [148, 123, 107]),
        // BT.709 red at 100 nits, as HDR10 shows it.
        ([341, 632, 623], [186, 0, 0]),
        // BT.709 green at 50 nits.
        ([387, 365, 588], [4, 136, 1]),
        // BT.709 blue at 20 nits.
        ([214, 418, 304], [0, 0, 88]),
        // Yellow at 400 nits, well over SDR white.
        ([866, 423, 632], [255, 255, 0]),
    ];
    let codes: Vec<_> = frame.iter().map(|(codes, _)| *codes).collect();
    let rendered = render_codes(&codes, 4);
    for ((codes, want), got) in frame.iter().zip(rendered) {
        let close = want.iter().zip(got).all(|(&a, b)| a.abs_diff(b) <= 1);
        assert!(close, "{codes:?}: want {want:?}, got {got:?}");
    }
}

#[test]
fn chroma_tints_the_way_ipt_says() {
    // Reference white grey, then its P (red against green) and T (yellow
    // against blue) codes moved either way.
    let [grey, more_p, less_p, more_t, less_t] = render_codes(
        &[
            [833, 464, 464],
            [833, 504, 464],
            [833, 424, 464],
            [833, 464, 504],
            [833, 464, 424],
        ],
        5,
    )[..] else {
        unreachable!()
    };
    let [r, g, b] = grey.map(i32::from);
    assert!((r - g).abs() <= 2 && (g - b).abs() <= 2, "{grey:?}");
    let most = |c: [u8; 3]| (0..3).max_by_key(|&i| c[i]).unwrap();
    let least = |c: [u8; 3]| (0..3).min_by_key(|&i| c[i]).unwrap();
    assert_eq!((most(more_p), least(more_p)), (0, 1), "redder: {more_p:?}");
    assert_eq!((most(less_p), least(less_p)), (1, 0), "greener: {less_p:?}");
    assert_eq!(least(more_t), 2, "yellower: {more_t:?}");
    assert_eq!(most(less_t), 2, "bluer: {less_t:?}");
}
