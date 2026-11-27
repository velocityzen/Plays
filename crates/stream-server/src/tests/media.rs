use std::path::PathBuf;
use std::sync::Arc;

use ffmpeg::format::context::Input;

use super::*;

/// `name`, in `tests/fixtures`.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

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

/// FFmpeg's own matrices, as its demuxers make them, come back as the
/// turns and mirroring they ask for. `av_display_rotation_set` takes
/// degrees clockwise, where ffprobe gives them counterclockwise.
#[test]
fn reads_display_matrices_as_turns_and_mirroring() {
    let read = |rotation: f64, flipped: bool| {
        let mut matrix = [0i32; 9];
        // SAFETY: both write nine i32s.
        unsafe {
            ffi::av_display_rotation_set(matrix.as_mut_ptr(), rotation);
            ffi::av_display_matrix_flip(matrix.as_mut_ptr(), flipped.into(), 0);
        }
        orientation(&matrix)
    };
    assert_eq!(read(0.0, false), Some((0, false)));
    assert_eq!(read(90.0, false), Some((1, false)));
    assert_eq!(read(-90.0, false), Some((3, false)));
    assert_eq!(read(180.0, false), Some((2, false)));
    assert_eq!(read(88.0, false), Some((1, false)));
    // The flip comes after the turn: mirrored first, it turns the other
    // way.
    assert_eq!(read(0.0, true), Some((0, true)));
    assert_eq!(read(90.0, true), Some((3, true)));
    assert_eq!(read(180.0, true), Some((2, true)));
    assert_eq!(orientation(&[0; 9]), None);
}

/// A video's file asks for it turned as its display matrix says, and
/// shown at its pixels' shape, its container's before its coded
/// pictures'.
#[test]
fn reads_how_its_file_asks_a_video_shown() {
    crate::init();
    let read = |name: &str| {
        let source = Source::File(fixture(name));
        let input = open(&source, Arc::default()).unwrap();
        let info = MediaInfo::read(&input, &source.location());
        let video = info.video(info.first(true).unwrap()).unwrap();
        (video.own, video.container_aspect)
    };
    let wide = |(own, container): (Adjustments, bool)| {
        let aspect = own.aspect.expect("a shape");
        assert!((aspect - 16.0 / 9.0).abs() < 1e-9, "{aspect}");
        (
            Adjustments {
                aspect: None,
                ..own
            },
            container,
        )
    };
    let plain = Adjustments::default();
    assert_eq!(read("clip.mp4"), (plain, false));
    let turned = Adjustments { turns: 3, ..plain };
    assert_eq!(read("shapes/turned.mp4"), (turned, false));
    assert_eq!(wide(read("shapes/wide.mp4")), (plain, false));
    assert_eq!(wide(read("shapes/wide.mkv")), (plain, true));
}

/// No shape from a picture of no height, nor from a shape unsaid or not
/// a shape; the container's square pixels where the coded pictures say
/// nothing are theirs too, kept by a copy.
#[test]
fn reads_no_shape_from_too_little() {
    crate::init();
    let mut input = open(&Source::File(fixture("clip.mp4")), Arc::default()).unwrap();
    let index = input.streams().best(Type::Video).unwrap().index();
    let mut read = |height, container: (i32, i32), coded: (i32, i32)| {
        let ratio = |(num, den)| ffi::AVRational { num, den };
        let mut stream = input.stream_mut(index).unwrap();
        // SAFETY: the stream is ours and open, and these plain numbers.
        unsafe {
            let raw = stream.as_mut_ptr();
            (*(*raw).codecpar).height = height;
            (*raw).sample_aspect_ratio = ratio(container);
            (*(*raw).codecpar).sample_aspect_ratio = ratio(coded);
        }
        let info = video_info(&input.stream(index).unwrap());
        (info.own.aspect, info.container_aspect)
    };
    assert_eq!(read(120, (1, 1), (0, 1)), (None, false));
    assert_eq!(read(120, (0, 1), (0, 1)), (None, false));
    assert_eq!(read(120, (-4, 3), (4, -3)), (None, false));
    assert_eq!(read(0, (4, 3), (4, 3)), (None, false));
    assert!(!read(120, (0, 1), (4, 3)).1, "the coded pictures say it");
    let coded = read(120, (0, 1), (4, 3)).0.expect("the coded shape");
    assert!((coded - 16.0 / 9.0).abs() < 1e-9, "{coded}");
    // Pixels at most 16 times as wide, or as narrow, as square, as the
    // player takes them.
    let wide = read(120, (100, 1), (0, 1)).0.expect("a wide shape");
    assert!((wide - 160.0 * 16.0 / 120.0).abs() < 1e-9, "{wide}");
    let narrow = read(120, (0, 1), (1, 100)).0.expect("a narrow shape");
    assert!((narrow - 160.0 / 16.0 / 120.0).abs() < 1e-9, "{narrow}");
}

/// Opened `name`, in `tests/fixtures`, and its video's index.
fn opened(name: &str) -> (Input, usize) {
    crate::init();
    let input = open(&Source::File(fixture(name)), Arc::default()).unwrap();
    let index = input.streams().best(Type::Video).unwrap().index();
    (input, index)
}

/// Dolby Vision's enhancement layer in a track of its own, its base layer
/// in another, is read as one alone, as the player takes it, and never
/// picked to play.
#[test]
fn reads_an_enhancement_layer_alone() {
    let (mut input, index) = opened("clip-odd-colors.mkv");
    let mut alone = |enhancement: u8, base: u8| {
        let mut stream = input.stream_mut(index).unwrap();
        // SAFETY: the stream is ours and open; the side data FFmpeg
        // allocates is written within its size.
        unsafe {
            let parameters = (*stream.as_mut_ptr()).codecpar;
            ffi::av_packet_side_data_remove(
                (*parameters).coded_side_data,
                &raw mut (*parameters).nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
            );
            let made = ffi::av_packet_side_data_new(
                &raw mut (*parameters).coded_side_data,
                &raw mut (*parameters).nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
                24,
                0,
            );
            assert!(!made.is_null());
            std::ptr::write_bytes((*made).data, 0, 24);
            (*made).data.add(2).write(7);
            (*made).data.add(5).write(enhancement);
            (*made).data.add(6).write(base);
        }
        video_info(&input.stream(index).unwrap()).layer_only
    };
    assert!(alone(1, 0));
    assert!(!alone(1, 1), "both layers in one track");
    assert!(!alone(0, 1), "the base layer's track");
    assert!(!alone(0, 0));

    let video = |index, default, layer_only| StreamInfo {
        index,
        kind: StreamKind::Video(VideoInfo {
            layer_only,
            ..video_info(&input.stream(0).unwrap())
        }),
        language: None,
        title: None,
        default,
    };
    let media = MediaInfo {
        container: Container::Mp4,
        duration: None,
        start: 0.0,
        bit_rate: None,
        streams: vec![video(0, true, true), video(1, false, false)],
    };
    assert_eq!(media.first(true), Some(1));
}

/// Subtitles are text where FFmpeg's table says so, as the player asks it:
/// TTML, SAMI, MicroDVD and broadcast captions too, never pictures.
#[test]
fn takes_subtitles_for_text_as_ffmpeg_does() {
    for id in [
        Id::SUBRIP,
        Id::ASS,
        Id::MOV_TEXT,
        Id::WEBVTT,
        Id::TTML,
        Id::SAMI,
        Id::MICRODVD,
        Id::EIA_608,
    ] {
        assert!(is_text(id), "{id:?}");
    }
    for id in [
        Id::HDMV_PGS_SUBTITLE,
        Id::DVD_SUBTITLE,
        Id::DVB_SUBTITLE,
        Id::XSUB,
        Id::H264,
        Id::None,
    ] {
        assert!(!is_text(id), "{id:?}");
    }
}

/// A stream's frame rate is its average, else the rate FFmpeg guesses,
/// from 1 to 480 a second, as the player takes it.
#[test]
fn reads_the_frame_rate_as_the_player_does() {
    let (mut input, index) = opened("clip.mp4");
    let mut rate = |average: (i32, i32), guessed: (i32, i32)| {
        let ratio = |(num, den)| ffi::AVRational { num, den };
        let mut stream = input.stream_mut(index).unwrap();
        // SAFETY: the stream is ours and open, and these plain numbers.
        unsafe {
            (*stream.as_mut_ptr()).avg_frame_rate = ratio(average);
            (*stream.as_mut_ptr()).r_frame_rate = ratio(guessed);
        }
        let stream = input.stream(index).unwrap();
        let rate = frame_rate(&stream).map(f64::from);
        assert_eq!(video_info(&stream).frame_rate, rate, "as the video says");
        rate
    };
    assert_eq!(rate((25, 1), (50, 1)), Some(25.0));
    assert_eq!(rate((0, 0), (30_000, 1001)), Some(30_000.0 / 1001.0));
    assert_eq!(rate((0, 1), (25, 1)), Some(25.0));
    assert_eq!(rate((25, 0), (30, 1)), Some(30.0));
    assert_eq!(rate((0, 1), (24, 0)), None);
    assert_eq!(rate((480, 1), (0, 0)), Some(480.0));
    assert_eq!(rate((1, 1), (0, 0)), Some(1.0));
    // Not plausible, whatever the other says.
    assert_eq!(rate((481, 1), (25, 1)), None);
    assert_eq!(rate((1, 2), (25, 1)), None);
    assert_eq!(rate((0, 0), (90_000, 1)), None);
}

/// A stream tagged as in no language, or one unknown, has none.
#[test]
fn reads_no_language_from_none_or_an_unknown_one() {
    let (mut input, index) = opened("clip.mp4");
    let mut language = |code: &std::ffi::CStr| {
        let mut stream = input.stream_mut(index).unwrap();
        // SAFETY: the stream is ours and open; FFmpeg copies the strings.
        unsafe {
            let raw = stream.as_mut_ptr();
            ffi::av_dict_set(
                &raw mut (*raw).metadata,
                c"language".as_ptr(),
                code.as_ptr(),
                0,
            );
        }
        let info = MediaInfo::read(&input, "clip.mp4");
        info.stream(index).unwrap().language.clone()
    };
    assert_eq!(language(c"fra").as_deref(), Some("fra"));
    assert_eq!(language(c"und"), None);
    assert_eq!(language(c"zxx"), None, "no linguistic content");
}
