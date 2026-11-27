use std::fs;
use std::path::{Path, PathBuf};

use ffmpeg_next::{Rational, codec, ffi};
use serde_json::{Value, json};

use super::*;
use crate::media::tests::made::{kept, made_from, made_in, media};
use crate::media::tests::{http, layered_media};

const CUES: &str = "1\n00:00:00,000 --> 00:00:01,000\nHello\n";

const CHAPTERS: &str = ";FFMETADATA1\ntitle=Probed\n\
    [CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=One\n\
    [CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=Two\n";

/// A Matroska file of each kind of stream, titled and in two chapters:
/// H.264 in BT.709, FLAC in French titled "Commentary", SubRip in English
/// and forced, and a text file kept in it; with English subtitles beside
/// it.
pub(crate) fn everything() -> PathBuf {
    made_in(&kept(), "probe/everything.en.srt", CUES, |out| {
        fs::write(out, CUES).map_err(|err| err.to_string())
    });
    made_from(
        "ffmpeg",
        "probe/everything.mkv",
        &[
            ("cues.srt", CUES),
            ("chapters.txt", CHAPTERS),
            ("notes.txt", "Notes"),
        ],
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=2",
            "-i",
            "cues.srt",
            "-i",
            "chapters.txt",
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-map_metadata",
            "3",
            "-map_chapters",
            "3",
            "-attach",
            "notes.txt",
            "-metadata:s:3",
            "mimetype=text/plain",
            "-vf",
            "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "flac",
            "-ac",
            "2",
            "-c:s",
            "srt",
            "-metadata:s:a:0",
            "language=fre",
            "-metadata:s:a:0",
            "title=Commentary",
            "-metadata:s:s:0",
            "language=eng",
            "-disposition:s:0",
            "forced+hearing_impaired",
        ],
    )
}

/// How `pq` is made.
const PQ: &[&str] = &[
    "-f",
    "lavfi",
    "-i",
    "testsrc=size=320x240:rate=24000/1001:duration=1",
    "-vf",
    "setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc:range=tv",
    "-c:v",
    "libx265",
    "-x265-params",
    "log-level=error",
    "-pix_fmt",
    "yuv420p10le",
    "-timecode",
    "01:00:00:00",
];

/// A second of HEVC Main 10 in BT.2020 and PQ, 24000/1001 frames a
/// second, in a QuickTime file with a timecode.
fn pq() -> PathBuf {
    media("probe/pq.mov", PQ)
}

/// H.264 in full range, turned a quarter turn counterclockwise and
/// mirrored, as its display matrix says.
fn turned() -> PathBuf {
    media(
        "probe/turned.mp4",
        &[
            "-noautorotate",
            "-display_rotation",
            "90",
            "-display_hflip",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=1",
            "-vf",
            "setparams=range=pc",
            "-c:v",
            "libx264",
        ],
    )
}

/// `pq`'s video in MP4, with HDR10's metadata, mastered at 1000
/// nits, MaxCLL 1000 and MaxFALL 400, and Dolby Vision 8.1's
/// configuration.
fn hdr10() -> PathBuf {
    let pq = pq();
    made_in(
        &kept(),
        "probe/hdr10.mp4",
        &format!("{PQ:?} in MP4, mastered with BT.2020's primaries, and Dolby Vision 8.1"),
        |out| with_hdr_metadata(&pq, out).map_err(|err| err.to_string()),
    )
}

/// `from`'s video into `to`, with HDR10's metadata and Dolby Vision 8.1's
/// configuration.
fn with_hdr_metadata(from: &Path, to: &Path) -> Result<(), ffmpeg_next::Error> {
    use ffmpeg_next::{codec, media::Type};

    let mut input = ffmpeg_safe::input_file(from)?;
    let mut output = ffmpeg_next::format::output(to)?;
    let (index, time_base, parameters) = {
        let stream = input.streams().best(Type::Video).unwrap();
        (stream.index(), stream.time_base(), stream.parameters())
    };
    let mut stream = output.add_stream(ffmpeg_next::encoder::find(codec::Id::None))?;
    stream.set_parameters(parameters);
    stream.set_time_base(time_base);
    // SAFETY: all numbers, which nothing is.
    let mut display: hdr::sys::MasteringDisplay = unsafe { std::mem::zeroed() };
    display.max_luminance = ffi::AVRational { num: 1000, den: 1 };
    display.min_luminance = ffi::AVRational {
        num: 1,
        den: 10_000,
    };
    display.has_luminance = 1;
    // BT.2020's, in fifty thousandths, as MP4 keeps them only with its
    // luminance.
    let at = |x, y| [x, y].map(|num| ffi::AVRational { num, den: 50_000 });
    display.display_primaries = [at(35_400, 14_600), at(8_500, 39_850), at(6_550, 2_300)];
    display.white_point = at(15_635, 16_450);
    display.has_primaries = 1;
    let light = hdr::sys::ContentLightLevel {
        max_cll: 1000,
        max_fall: 400,
    };
    // Version 1.0, profile 8, level 6, an RPU and the base layer, which
    // is HDR10.
    let dolby_vision = [1u8, 0, 8, 6, 1, 0, 1, 1, 0];
    // SAFETY: the stream's parameters are valid; FFmpeg makes the side
    // data as big as asked, owned by them, and each is written as the
    // struct it is.
    unsafe {
        let raw = (*stream.as_mut_ptr()).codecpar;
        (*raw).codec_tag = 0;
        let add = |kind, data: *const u8, size: usize| {
            let side = ffi::av_packet_side_data_new(
                &mut (*raw).coded_side_data,
                &mut (*raw).nb_coded_side_data,
                kind,
                size,
                0,
            );
            assert!(!side.is_null());
            std::ptr::copy_nonoverlapping(data, (*side).data, size);
        };
        use ffi::AVPacketSideDataType::*;
        add(
            AV_PKT_DATA_MASTERING_DISPLAY_METADATA,
            (&raw const display).cast(),
            size_of_val(&display),
        );
        add(
            AV_PKT_DATA_CONTENT_LIGHT_LEVEL,
            (&raw const light).cast(),
            size_of_val(&light),
        );
        add(
            AV_PKT_DATA_DOVI_CONF,
            dolby_vision.as_ptr(),
            dolby_vision.len(),
        );
    }
    // MP4 keeps a Dolby Vision configuration only so.
    // SAFETY: setting a field of a valid context.
    unsafe { (*output.as_mut_ptr()).strict_std_compliance = ffi::FF_COMPLIANCE_UNOFFICIAL };
    output.write_header()?;
    let to_base = output.stream(0).unwrap().time_base();
    for (stream, mut packet) in input.packets() {
        if stream.index() == index {
            packet.set_stream(0);
            packet.rescale_ts(time_base, to_base);
            packet.set_position(-1);
            packet.write_interleaved(&mut output)?;
        }
    }
    output.write_trailer()
}

/// Interlaced H.264, the top field first, of 4:3 pixels, in MPEG-TS,
/// which keeps no shape of its own.
fn interlaced() -> PathBuf {
    media(
        "probe/interlaced.ts",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=1",
            "-vf",
            "setsar=4/3",
            "-c:v",
            "libx264",
            "-flags",
            "+ildct+ilme",
            "-x264opts",
            "tff=1",
        ],
    )
}

/// H.264 whose pictures say their pixels are 4:3, in a Matroska file
/// that shows it twice as wide as high: of 3:2 pixels.
fn wide() -> PathBuf {
    media(
        "probe/wide.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=1",
            "-aspect",
            "2:1",
            "-c:v",
            "libx264",
            "-bsf:v",
            "h264_metadata=sample_aspect_ratio=4/3",
        ],
    )
}

/// MP3 with a PNG of its cover.
fn covered() -> PathBuf {
    media(
        "probe/covered.mp3",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=1",
            "-f",
            "lavfi",
            "-i",
            "color=red:size=64x64:duration=1",
            "-map",
            "0",
            "-map",
            "1",
            "-frames:v",
            "1",
            "-c:a",
            "libmp3lame",
            "-c:v",
            "png",
            "-disposition:v",
            "attached_pic",
        ],
    )
}

/// What `path` holds, as JSON.
fn probed(path: &Path) -> Value {
    serde_json::to_value(Probe::of(&Source::File(path.to_owned())).unwrap()).unwrap()
}

/// Stream `index` of what `probe` read.
fn stream(probe: &Value, index: usize) -> &Value {
    &probe["streams"][index]
}

#[test]
fn a_files_format_tags_chapters_and_subtitles_beside_it_are_read() {
    let path = everything();
    let probe = probed(&path);
    assert_eq!(probe["source"], path.display().to_string());
    assert_eq!(probe["format"], "Matroska / WebM");
    assert_eq!(probe["duration"], 2.0);
    assert_eq!(probe["size"], fs::metadata(&path).unwrap().len());
    assert!(probe["bit_rate"].as_u64().is_some_and(|rate| rate > 0));
    assert_eq!(probe["tags"]["title"], "Probed");
    assert_eq!(
        probe["chapters"],
        json!([
            {"start": 0.0, "end": 1.0, "title": "One"},
            {"start": 1.0, "end": 2.0, "title": "Two"},
        ])
    );
    assert_eq!(
        probe["subtitle_files"],
        json!([{
            "path": path.with_file_name("everything.en.srt").display().to_string(),
            "codec": "subrip",
            "language": "en",
            "title": null,
            "forced": false,
            "hearing_impaired": false,
        }])
    );
}

#[test]
fn each_kind_of_stream_in_a_file_is_read() {
    let probe = probed(&everything());
    assert_eq!(
        *stream(&probe, 0),
        json!({
            "index": 0,
            "kind": "video",
            "codec": "h264",
            "codec_name": "H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10",
            "codec_tag": null,
            "profile": "High",
            "bit_rate": null,
            "language": null,
            "title": null,
            "default": false,
            "forced": false,
            "hearing_impaired": false,
            "cover": false,
            "video": {
                "width": 320,
                "height": 240,
                "display_width": 320,
                "display_height": 240,
                "pixel_aspect": null,
                "frame_rate": 25.0,
                "pixel_format": "yuv420p",
                "bit_depth": 8,
                "matrix": "bt709",
                "primaries": "bt709",
                "transfer": "bt709",
                "range": "limited",
                "hdr": null,
                "dolby_vision": null,
                "mastering_peak": null,
                "max_cll": null,
                "max_fall": null,
                "rotation": 0,
                "mirrored": false,
                "first_field": null,
            },
        })
    );
    assert_eq!(
        *stream(&probe, 1),
        json!({
            "index": 1,
            "kind": "audio",
            "codec": "flac",
            "codec_name": "FLAC (Free Lossless Audio Codec)",
            "codec_tag": null,
            "profile": null,
            "bit_rate": null,
            "language": "fre",
            "title": "Commentary",
            "default": false,
            "forced": false,
            "hearing_impaired": false,
            "cover": false,
            "audio": {
                "sample_rate": 48000,
                "channels": 2,
                "layout": "stereo",
                "sample_format": "s16",
                "bits": 16,
            },
        })
    );
    assert_eq!(
        *stream(&probe, 2),
        json!({
            "index": 2,
            "kind": "subtitle",
            "codec": "subrip",
            "codec_name": "SubRip subtitle",
            "codec_tag": null,
            "profile": null,
            "bit_rate": null,
            "language": "eng",
            "title": null,
            "default": false,
            "forced": true,
            "hearing_impaired": true,
            "cover": false,
            "subtitle": {"text": true},
        })
    );
    // A file kept in it, named as it is.
    assert_eq!(stream(&probe, 3)["kind"], "attachment");
    assert_eq!(stream(&probe, 3)["codec_name"], Value::Null);
    assert_eq!(stream(&probe, 3)["title"], "notes.txt");
    assert_eq!(stream(&probe, 4), &Value::Null);
}

#[test]
fn subtitles_are_text_or_pictures() {
    let dvd = probed(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clip-dvdsub.mkv"));
    let subtitles = dvd["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stream| stream["kind"] == "subtitle")
        .unwrap();
    assert_eq!(
        (&subtitles["codec"], &subtitles["subtitle"]),
        (&json!("dvd_subtitle"), &json!({"text": false}))
    );
}

#[test]
fn hdr_its_light_and_dolby_vision_are_read() {
    let pq = probed(&pq());
    let video = &stream(&pq, 0)["video"];
    assert_eq!(stream(&pq, 0)["codec_tag"], "hev1");
    assert_eq!(stream(&pq, 0)["profile"], "Main 10");
    assert!(
        stream(&pq, 0)["bit_rate"]
            .as_u64()
            .is_some_and(|rate| rate > 0)
    );
    assert_eq!(stream(&pq, 0)["default"], true);
    assert!((video["frame_rate"].as_f64().unwrap() - 24_000.0 / 1001.0).abs() < 1e-9);
    assert_eq!(video["pixel_format"], "yuv420p10le");
    assert_eq!(video["bit_depth"], 10);
    let colours = [&video["matrix"], &video["primaries"], &video["transfer"]];
    assert_eq!(colours, ["bt2020nc", "bt2020", "smpte2084"]);
    assert_eq!(video["hdr"], "HDR10");
    assert_eq!(video["dolby_vision"], Value::Null);
    // Its timecode.
    assert_eq!(stream(&pq, 1)["kind"], "data");
    assert_eq!(stream(&pq, 1)["codec"], "none");
    assert_eq!(stream(&pq, 1)["codec_tag"], "tmcd");

    let hdr10 = probed(&hdr10());
    let video = &stream(&hdr10, 0)["video"];
    assert_eq!(video["hdr"], "HDR10");
    assert_eq!(video["mastering_peak"], 1000.0);
    assert_eq!(video["max_cll"], 1000);
    assert_eq!(video["max_fall"], 400);
    assert_eq!(
        video["dolby_vision"],
        json!({
            "name": "8.1",
            "profile": 8,
            "level": 6,
            "compatibility": 1,
            "base_layer": true,
            "enhancement_layer": false,
        })
    );

    // Profile 7's enhancement layer in a track of its own, then its base
    // layer.
    let layered = probed(&layered_media());
    assert_eq!(
        stream(&layered, 0)["video"]["dolby_vision"],
        json!({
            "name": "7",
            "profile": 7,
            "level": 6,
            "compatibility": 6,
            "base_layer": false,
            "enhancement_layer": true,
        })
    );
    assert_eq!(stream(&layered, 1)["video"]["dolby_vision"], Value::Null);
}

#[test]
fn how_pictures_show_is_read_their_shape_turn_and_fields() {
    let turned = probed(&turned());
    let video = &stream(&turned, 0)["video"];
    assert_eq!(
        (&video["rotation"], &video["mirrored"]),
        (&json!(90), &json!(true))
    );
    assert_eq!(video["range"], "full");
    assert_eq!(video["first_field"], Value::Null);

    // Their pixels' shape, from the pictures where the file keeps none.
    let interlaced = probed(&interlaced());
    let video = &stream(&interlaced, 0)["video"];
    assert_eq!(video["first_field"], "top");
    assert_eq!(video["pixel_aspect"], "4:3");
    assert_eq!(
        (&video["display_width"], &video["display_height"]),
        (&json!(427), &json!(240))
    );
    assert_eq!(video["range"], Value::Null);
    let colours = [&video["matrix"], &video["primaries"], &video["transfer"]];
    assert_eq!(colours, [&Value::Null; 3], "none it doesn't say");

    // And from the file, over theirs.
    let wide = probed(&wide());
    let video = &stream(&wide, 0)["video"];
    assert_eq!(video["pixel_aspect"], "3:2");
    assert_eq!(
        (&video["display_width"], &video["display_height"]),
        (&json!(240), &json!(120))
    );

    let covered = probed(&covered());
    assert_eq!(stream(&covered, 1)["kind"], "video");
    assert_eq!(stream(&covered, 1)["codec"], "png");
    assert_eq!(stream(&covered, 1)["cover"], true);
    assert_eq!(stream(&covered, 0)["cover"], false);
}

#[test]
fn a_stream_has_no_size_nor_files_beside_it_and_a_file_may_have_no_duration() {
    let dir = everything().parent().unwrap().to_owned();
    let address = http::serve(dir, None);
    let source = Source::Url(format!("http://user:secret@{address}/everything.mkv"));
    let probe = Probe::of(&source).unwrap();
    assert_eq!(probe.source, format!("http://{address}/everything.mkv"));
    assert_eq!(probe.format, "Matroska / WebM");
    assert_eq!(probe.size, None);
    assert!(probe.subtitle_files.is_empty());

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clip-no-duration.mkv");
    assert_eq!(probed(&fixture)["duration"], Value::Null);
    let missing = Probe::of(&Source::File("missing.mkv".into())).unwrap_err();
    assert_eq!(
        format!("{missing:#}"),
        "could not open missing.mkv: No such file or directory"
    );
}

#[test]
fn hdr_is_named_as_players_without_dolby_vision_show_it() {
    use color::TransferCharacteristic::*;
    let dolby_vision = |profile, compatibility| {
        DolbyVision::of(Configuration {
            profile,
            level: 6,
            compatibility,
            base: true,
            enhancement: false,
        })
    };
    assert_eq!(hdr_name(SMPTE2084, None), Some("HDR10"));
    assert_eq!(hdr_name(ARIB_STD_B67, None), Some("HLG"));
    assert_eq!(hdr_name(BT709, None), None);
    assert_eq!(
        hdr_name(SMPTE2084, Some(&dolby_vision(8, 1))),
        Some("HDR10")
    );
    assert_eq!(
        hdr_name(ARIB_STD_B67, Some(&dolby_vision(8, 4))),
        Some("HLG")
    );
    // Profile 5's base layer, which only Dolby Vision shows.
    assert_eq!(hdr_name(SMPTE2084, Some(&dolby_vision(5, 0))), None);

    let names: Vec<String> = [(5, 0), (7, 6), (8, 1), (8, 4), (9, 2), (10, 0), (10, 4)]
        .map(|(profile, compatibility)| dolby_vision(profile, compatibility).name)
        .into();
    assert_eq!(names, ["5", "7", "8.1", "8.4", "9.2", "10.0", "10.4"]);
}

/// Parameters with side data of `kind` holding `value`.
fn with_side_data<T>(kind: ffi::AVPacketSideDataType, value: T) -> codec::Parameters {
    let mut parameters = codec::Parameters::new();
    // SAFETY: FFmpeg makes the side data as big as asked, owned by the
    // parameters.
    unsafe {
        let raw = parameters.as_mut_ptr();
        let side = ffi::av_packet_side_data_new(
            &mut (*raw).coded_side_data,
            &mut (*raw).nb_coded_side_data,
            kind,
            size_of::<T>(),
            0,
        );
        assert!(!side.is_null());
        std::ptr::write_unaligned((*side).data.cast::<T>(), value);
    }
    parameters
}

#[test]
fn light_is_read_where_it_says_it_is_and_has_some() {
    use ffi::AVPacketSideDataType::*;
    assert_eq!(light(&codec::Parameters::new()), (None, None, None));
    // SAFETY: all numbers, which nothing is.
    let mut display: hdr::sys::MasteringDisplay = unsafe { std::mem::zeroed() };
    display.max_luminance = ffi::AVRational { num: 4000, den: 1 };
    let unsaid = with_side_data(AV_PKT_DATA_MASTERING_DISPLAY_METADATA, display);
    assert_eq!(light(&unsaid).0, None, "its luminance isn't said");
    let unlit = with_side_data(
        AV_PKT_DATA_CONTENT_LIGHT_LEVEL,
        hdr::sys::ContentLightLevel {
            max_cll: 0,
            max_fall: 0,
        },
    );
    assert_eq!(light(&unlit), (None, None, None), "none: unknown");
    let short = with_side_data(AV_PKT_DATA_CONTENT_LIGHT_LEVEL, 1000_u32);
    assert_eq!(light(&short), (None, None, None), "too little to be one");
}

#[test]
fn formats_and_codes_are_named_as_ffmpeg_names_them() {
    use ffi::AVPixelFormat::*;
    assert_eq!(
        pixel_format(AV_PIX_FMT_YUV422P12LE as c_int),
        (Some("yuv422p12le".to_owned()), Some(12))
    );
    // A hardware frame's has no bits.
    assert_eq!(
        pixel_format(AV_PIX_FMT_VIDEOTOOLBOX as c_int),
        (Some("videotoolbox_vld".to_owned()), None)
    );
    assert_eq!(pixel_format(-1), (None, None));
    assert_eq!(pixel_format(100_000), (None, None));

    use ffi::AVSampleFormat::*;
    assert_eq!(sample_format(AV_SAMPLE_FMT_FLTP as c_int), Some("fltp"));
    assert_eq!(sample_format(AV_SAMPLE_FMT_S64P as c_int), Some("s64p"));
    assert_eq!(sample_format(-1), None);

    use ffi::AVFieldOrder::*;
    let fields = [
        AV_FIELD_UNKNOWN,
        AV_FIELD_PROGRESSIVE,
        AV_FIELD_TT,
        AV_FIELD_BB,
        AV_FIELD_TB,
        AV_FIELD_BT,
    ]
    .map(|order| first_field(order as u32));
    assert_eq!(
        fields,
        [
            None,
            None,
            Some("top"),
            Some("bottom"),
            Some("bottom"),
            Some("top")
        ]
    );

    let code = |text: &[u8; 4]| fourcc(u32::from_le_bytes(*text));
    assert_eq!(code(b"tmcd").as_deref(), Some("tmcd"));
    assert_eq!(code(b"ac3 ").as_deref(), Some("ac3"));
    assert_eq!(code(b"\x01abc"), None);
    assert_eq!(fourcc(0), None);

    assert_eq!(ratio(Rational::new(3, 2)), Some(1.5));
    for (numerator, denominator) in [(0, 1), (1, 0), (-1, -1), (0, 0)] {
        assert_eq!(ratio(Rational::new(numerator, denominator)), None);
    }
}
