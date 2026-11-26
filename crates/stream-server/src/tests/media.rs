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
/// layer's compatibility says: HDR10 for profiles 7, 8.1 and 10.1, HLG for
/// 8.4, SDR for 8.2. With none (profiles 5, 10.0 and 20) it's IPT, which
/// is neither, whatever it's tagged. A configuration too short to say
/// leaves it to the colors: IPT tagged so (profile 20) or not as HDR
/// (profile 5). Its profile is the configuration's, where it says.
#[test]
fn reads_dolby_visions_base_layer_as_hdr10_or_hlg() {
    use ffi::AVColorSpace::*;
    use ffi::AVColorTransferCharacteristic::*;
    crate::init();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/clip-odd-colors.mkv");
    let mut input = open(&Source::File(path), Arc::default()).unwrap();
    let index = input.streams().best(Type::Video).unwrap().index();
    // With `Some((size, profile, compatibility))`, a Dolby Vision
    // configuration that long, saying those where it's long enough.
    let mut read = |transfer, space, configuration: Option<(usize, u8, u8)>| {
        let mut stream = input.stream_mut(index).unwrap();
        // SAFETY: the stream is ours and open; its parameters take these
        // values FFmpeg names, and side data FFmpeg allocates, written
        // within its size.
        unsafe {
            let parameters = (*stream.as_mut_ptr()).codecpar;
            (*parameters).color_trc = transfer;
            (*parameters).color_space = space;
            ffi::av_packet_side_data_remove(
                (*parameters).coded_side_data,
                &raw mut (*parameters).nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
            );
            if let Some((size, profile, compatibility)) = configuration {
                let made = ffi::av_packet_side_data_new(
                    &raw mut (*parameters).coded_side_data,
                    &raw mut (*parameters).nb_coded_side_data,
                    ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
                    size,
                    0,
                );
                assert!(!made.is_null());
                std::ptr::write_bytes((*made).data, 0, size);
                if size > 2 {
                    (*made).data.add(2).write(profile);
                }
                if size > 7 {
                    (*made).data.add(7).write(compatibility);
                }
            }
        }
        let info = video_info(&input.stream(index).unwrap());
        (info.hdr, info.base_hdr, info.ipt, info.dolby_vision)
    };

    let (hdr10, hlg) = (Some(Hdr::Hdr10), Some(Hdr::Hlg));
    assert_eq!(
        read(AVCOL_TRC_SMPTE2084, AVCOL_SPC_BT2020_NCL, None),
        (hdr10, hdr10, false, None)
    );
    assert_eq!(
        read(AVCOL_TRC_ARIB_STD_B67, AVCOL_SPC_BT2020_NCL, None),
        (hlg, hlg, false, None)
    );
    assert_eq!(
        read(AVCOL_TRC_BT709, AVCOL_SPC_BT709, None),
        (None, None, false, None)
    );

    let dolby_vision = Some(Hdr::DolbyVision);
    let whole = |profile, compatibility| Some((24, profile, compatibility));
    assert_eq!(
        read(AVCOL_TRC_SMPTE2084, AVCOL_SPC_BT2020_NCL, whole(8, 1)),
        (dolby_vision, hdr10, false, Some(8))
    );
    assert_eq!(
        read(AVCOL_TRC_ARIB_STD_B67, AVCOL_SPC_BT2020_NCL, whole(8, 4)),
        (dolby_vision, hlg, false, Some(8))
    );
    assert_eq!(
        read(AVCOL_TRC_BT709, AVCOL_SPC_BT709, whole(8, 2)),
        (dolby_vision, None, false, Some(8))
    );
    for (transfer, space, profile) in [
        (AVCOL_TRC_SMPTE2084, AVCOL_SPC_IPT_C2, 20),
        (AVCOL_TRC_UNSPECIFIED, AVCOL_SPC_UNSPECIFIED, 5),
        (AVCOL_TRC_SMPTE2084, AVCOL_SPC_BT2020_NCL, 10),
    ] {
        assert_eq!(
            read(transfer, space, whole(profile, 0)),
            (dolby_vision, None, true, Some(profile)),
            "{transfer:?} {space:?}"
        );
    }

    let short = Some((4, 5, 0));
    assert_eq!(
        read(AVCOL_TRC_SMPTE2084, AVCOL_SPC_BT2020_NCL, short),
        (dolby_vision, hdr10, false, None)
    );
    for (transfer, space) in [
        (AVCOL_TRC_SMPTE2084, AVCOL_SPC_IPT_C2),
        (AVCOL_TRC_UNSPECIFIED, AVCOL_SPC_UNSPECIFIED),
    ] {
        assert_eq!(
            read(transfer, space, short),
            (dolby_vision, None, true, None),
            "{transfer:?} {space:?}"
        );
    }
}
