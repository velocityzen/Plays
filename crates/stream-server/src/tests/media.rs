use std::path::PathBuf;
use std::sync::Arc;

use super::*;

/// A track named in Windows-1252, not UTF-8, as a Matroska file's may be,
/// named as it says.
#[test]
fn reads_a_track_named_in_another_encoding() {
    crate::init();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/western-tags.mkv");
    let source = Source::File(path);
    let input = open(&source, Arc::default()).unwrap();
    let info = MediaInfo::read(&input, &source.location());
    let titles: Vec<_> = info
        .streams
        .iter()
        .map(|stream| stream.title.as_deref())
        .collect();
    assert_eq!(titles, [Some("Français")]);
}

/// Dolby Vision is HDR10 or HLG to a player without it, as its base
/// layer's colors say: HDR10 for profiles 7, 8.1 and 10.1, HLG for 8.4. In
/// IPT (profile 20), or saying nothing (profile 5), it's neither.
#[test]
fn reads_dolby_visions_base_layer_as_hdr10_or_hlg() {
    use ffi::AVColorSpace::*;
    use ffi::AVColorTransferCharacteristic::*;
    crate::init();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/clip-odd-colors.mkv");
    let mut input = open(&Source::File(path), Arc::default()).unwrap();
    let index = input.streams().best(Type::Video).unwrap().index();
    let mut read = |transfer, space, dolby_vision: bool| {
        let mut stream = input.stream_mut(index).unwrap();
        // SAFETY: the stream is ours and open; its parameters take these
        // values FFmpeg names, and side data FFmpeg allocates.
        unsafe {
            let parameters = (*stream.as_mut_ptr()).codecpar;
            (*parameters).color_trc = transfer;
            (*parameters).color_space = space;
            if dolby_vision && (*parameters).nb_coded_side_data == 0 {
                let made = ffi::av_packet_side_data_new(
                    &raw mut (*parameters).coded_side_data,
                    &raw mut (*parameters).nb_coded_side_data,
                    ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
                    24,
                    0,
                );
                assert!(!made.is_null());
            }
        }
        let info = video_info(&input.stream(index).unwrap());
        (info.hdr, info.base_hdr)
    };

    let (hdr10, hlg) = (Some(Hdr::Hdr10), Some(Hdr::Hlg));
    assert_eq!(
        read(AVCOL_TRC_SMPTE2084, AVCOL_SPC_BT2020_NCL, false),
        (hdr10, hdr10)
    );
    assert_eq!(
        read(AVCOL_TRC_ARIB_STD_B67, AVCOL_SPC_BT2020_NCL, false),
        (hlg, hlg)
    );
    assert_eq!(read(AVCOL_TRC_BT709, AVCOL_SPC_BT709, false), (None, None));

    let dolby_vision = Some(Hdr::DolbyVision);
    assert_eq!(
        read(AVCOL_TRC_SMPTE2084, AVCOL_SPC_BT2020_NCL, true),
        (dolby_vision, hdr10)
    );
    assert_eq!(
        read(AVCOL_TRC_ARIB_STD_B67, AVCOL_SPC_BT2020_NCL, true),
        (dolby_vision, hlg)
    );
    assert_eq!(
        read(AVCOL_TRC_SMPTE2084, AVCOL_SPC_IPT_C2, true),
        (dolby_vision, None)
    );
    assert_eq!(
        read(AVCOL_TRC_UNSPECIFIED, AVCOL_SPC_UNSPECIFIED, true),
        (dolby_vision, None)
    );
}
