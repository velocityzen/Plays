use ffmpeg_next::util::color;
use ffmpeg_next::{codec, ffi, frame};

use super::{Colors, Tags};

/// Numbers FFmpeg has no names for, in each.
const ODD: Tags = Tags {
    range: 3,
    space: 30,
    primaries: 15,
    transfer: 40,
};

/// HDR10's.
const HDR10: Tags = Tags {
    range: 1,
    space: 9,
    primaries: 9,
    transfer: 16,
};

#[test]
fn numbers_ffmpeg_has_no_names_for_are_unspecified() {
    assert_eq!(
        (ODD.range(), ODD.space(), ODD.primaries(), ODD.transfer()),
        (None, None, None, None)
    );
    assert_eq!(
        ODD.named(),
        Colors {
            range: color::Range::Unspecified,
            space: color::Space::Unspecified,
            primaries: color::Primaries::Unspecified,
            transfer: color::TransferCharacteristic::Unspecified,
        }
    );
    assert_eq!(
        (
            HDR10.range(),
            HDR10.space(),
            HDR10.primaries(),
            HDR10.transfer()
        ),
        (
            Some(ffi::AVColorRange::AVCOL_RANGE_MPEG),
            Some(ffi::AVColorSpace::AVCOL_SPC_BT2020_NCL),
            Some(ffi::AVColorPrimaries::AVCOL_PRI_BT2020),
            Some(ffi::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE2084),
        )
    );
    assert_eq!(
        HDR10.named(),
        Colors {
            range: color::Range::MPEG,
            space: color::Space::BT2020NCL,
            primaries: color::Primaries::BT2020,
            transfer: color::TransferCharacteristic::SMPTE2084,
        }
    );
}

#[test]
fn unspecified_is_no_name() {
    let unspecified = Tags {
        range: 0,
        space: 2,
        primaries: 2,
        transfer: 2,
    };
    assert_eq!(
        (
            unspecified.range(),
            unspecified.space(),
            unspecified.primaries(),
            unspecified.transfer()
        ),
        (None, None, None, None)
    );
}

#[test]
fn a_frames_tags_are_read_as_numbers() {
    let mut frame = frame::Video::empty();
    // SAFETY: the frame is the test's; its tags are written as the C ints
    // they are, as a demuxer passing a file's on would.
    unsafe {
        let raw = frame.as_mut_ptr();
        (&raw mut (*raw).color_range).cast::<u32>().write(ODD.range);
        (&raw mut (*raw).colorspace).cast::<u32>().write(ODD.space);
        (&raw mut (*raw).color_primaries)
            .cast::<u32>()
            .write(ODD.primaries);
        (&raw mut (*raw).color_trc)
            .cast::<u32>()
            .write(ODD.transfer);
    }
    assert_eq!(Tags::of_frame(&frame), ODD);
    assert_eq!(Colors::of_frame(&frame), ODD.named());
}

/// An H.264 stream's parameters, tagged `tags`.
fn parameters(tags: Tags) -> codec::Parameters {
    let mut parameters = codec::Parameters::new();
    // SAFETY: the parameters are the test's; the tags are written as the
    // C ints they are.
    unsafe {
        let raw = parameters.as_mut_ptr();
        (*raw).codec_type = ffi::AVMediaType::AVMEDIA_TYPE_VIDEO;
        (*raw).codec_id = ffi::AVCodecID::AV_CODEC_ID_H264;
        (&raw mut (*raw).color_range)
            .cast::<u32>()
            .write(tags.range);
        (&raw mut (*raw).color_space)
            .cast::<u32>()
            .write(tags.space);
        (&raw mut (*raw).color_primaries)
            .cast::<u32>()
            .write(tags.primaries);
        (&raw mut (*raw).color_trc)
            .cast::<u32>()
            .write(tags.transfer);
    }
    parameters
}

#[test]
fn a_streams_and_its_decoders_tags_are_read_as_numbers() {
    let odd = parameters(ODD);
    assert_eq!(Tags::of_parameters(&odd), ODD);
    assert_eq!(Colors::of_parameters(&odd), ODD.named());

    for tags in [ODD, HDR10] {
        let decoder = codec::Context::from_parameters(parameters(tags))
            .unwrap()
            .decoder()
            .video()
            .expect("an H.264 decoder");
        assert_eq!(Tags::of_decoder(&decoder), tags);
        assert_eq!(Colors::of_decoder(&decoder), tags.named());
    }
}
