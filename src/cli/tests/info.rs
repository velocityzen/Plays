use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::cli::Unanswered;
use crate::cli::tests::command_line;
use crate::media::chapters::Chapter;
use crate::media::probe::tests::everything;
use crate::media::probe::{AudioProbe, DolbyVision, SubtitleFile, SubtitleProbe};
use crate::media::tests::http;

/// A stream of `kind`, numbered 0, that says nothing of itself.
fn bare(kind: Kind) -> StreamProbe {
    StreamProbe {
        index: 0,
        kind,
        codec: "none".into(),
        codec_name: None,
        codec_tag: None,
        profile: None,
        bit_rate: None,
        language: None,
        title: None,
        default: false,
        forced: false,
        hearing_impaired: false,
        cover: false,
        video: None,
        audio: None,
        subtitle: None,
    }
}

/// Pictures `width` by `height` that say nothing else of themselves.
fn pictures(width: u32, height: u32) -> VideoProbe {
    VideoProbe {
        width,
        height,
        display_width: width,
        display_height: height,
        pixel_aspect: None,
        frame_rate: None,
        pixel_format: None,
        bit_depth: None,
        matrix: None,
        primaries: None,
        transfer: None,
        range: None,
        hdr: None,
        dolby_vision: None,
        mastering_peak: None,
        max_cll: None,
        max_fall: None,
        rotation: 0,
        mirrored: false,
        first_field: None,
    }
}

/// Sound that says nothing of itself.
fn sound() -> AudioProbe {
    AudioProbe {
        sample_rate: 0,
        channels: 0,
        layout: None,
        sample_format: None,
        bits: None,
    }
}

/// `video` as a line says it, after its size.
fn shown(video: VideoProbe) -> Vec<String> {
    video_parts(&video).split_off(1)
}

fn path(path: &std::path::Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn a_file_is_read_here_without_plays_as_text_or_as_json() {
    let file = everything();
    let probe = Probe::of(&Source::File(file.clone())).unwrap();

    let ran = command_line(None, Unanswered::Fails, &["info", path(&file)]);
    assert_eq!((ran.status, ran.err.as_str()), (0, ""), "{ran:?}");
    assert_eq!(ran.out, text(&probe));
    let beside = format!(
        "Beside it  {} · SRT · English",
        path(&file.with_extension("en.srt"))
    );
    for line in [
        path(&file),
        "Format     Matroska / WebM",
        "Duration   0:00:02.000",
        "Streams    0  Video       H.264 High · 320×240 · 25 fps · yuv420p · \
         bt709/bt709/bt709, limited range",
        "           1  Audio       FLAC · 48 kHz · stereo · s16 · 16-bit · French · “Commentary”",
        "           2  Subtitles   SRT · text · English · Forced · SDH",
        "           3  Attachment  Unknown · “notes.txt”",
        "Chapters   1  0:00:00.000–0:00:01.000  One",
        "           2  0:00:01.000–0:00:02.000  Two",
        &beside,
    ] {
        assert!(
            ran.out.lines().any(|shown| shown == line),
            "{line:?} in\n{}",
            ran.out
        );
    }
    let twice = command_line(None, Unanswered::Fails, &["info", path(&file), path(&file)]);
    assert_eq!(
        twice.out,
        format!("{}\n{}", text(&probe), text(&probe)),
        "a blank line between"
    );

    let value = serde_json::to_value(&probe).unwrap();
    let one = command_line(None, Unanswered::Fails, &["info", "--json", path(&file)]);
    assert_eq!(one.json(), value, "an object for one");
    let two = command_line(
        None,
        Unanswered::Fails,
        &["--json", "info", path(&file), path(&file)],
    );
    assert_eq!(two.json(), json!([value, value]), "a list for more");
}

#[test]
fn what_cant_be_read_is_said_and_the_rest_printed() {
    let file = everything();
    let probe = Probe::of(&Source::File(file.clone())).unwrap();
    let missing = "plays: could not open missing.mkv: No such file or directory";

    let ran = command_line(
        None,
        Unanswered::Fails,
        &["info", "missing.mkv", path(&file)],
    );
    assert_eq!(ran.failed(), (1, missing));
    assert_eq!(ran.out, text(&probe));
    let listed = command_line(
        None,
        Unanswered::Fails,
        &["info", "--json", path(&file), "missing.mkv"],
    );
    assert_eq!(listed.failed(), (1, missing));
    assert_eq!(
        listed.json(),
        json!([probe]),
        "a list, as two were asked for"
    );

    let none = command_line(None, Unanswered::Fails, &["info", "--json", "missing.mkv"]);
    assert_eq!((none.failed(), none.out.as_str()), ((1, missing), ""));
    let both = command_line(
        None,
        Unanswered::Fails,
        &["info", "missing.mkv", "gone.mkv"],
    );
    assert_eq!(
        (both.failed(), both.out.as_str()),
        (
            (
                1,
                &*format!("{missing}\nplays: could not open gone.mkv: No such file or directory")
            ),
            ""
        )
    );
}

#[test]
fn a_stream_is_read_without_its_password_and_has_no_size() {
    let address = http::serve(everything().parent().unwrap().to_owned(), None);
    let url = format!("http://user:secret@{address}/everything.mkv");
    let ran = command_line(None, Unanswered::Fails, &["info", &url]);
    assert_eq!(ran.status, 0, "{ran:?}");
    assert_eq!(
        ran.out.lines().next(),
        Some(&*format!("http://{address}/everything.mkv"))
    );
    assert!(
        ran.out.contains("\nFormat    Matroska / WebM\n"),
        "{}",
        ran.out
    );
    assert!(
        !ran.out.contains("Size") && !ran.out.contains("Beside it"),
        "{}",
        ran.out
    );
}

#[test]
fn a_probe_is_shown_under_headings_its_values_lined_up() {
    let mut film = bare(Kind::Video);
    film.codec = "hevc".into();
    film.profile = Some("Main 10".into());
    film.bit_rate = Some(20_000_000);
    film.default = true;
    film.video = Some(VideoProbe {
        frame_rate: Some(24000.0 / 1001.0),
        pixel_format: Some("yuv420p10le".into()),
        matrix: Some("bt2020nc"),
        primaries: Some("bt2020"),
        transfer: Some("smpte2084"),
        range: Some("limited"),
        hdr: Some("HDR10"),
        ..pictures(3840, 2160)
    });
    let mut signs = bare(Kind::Subtitle);
    signs.index = 10;
    signs.codec = "hdmv_pgs_subtitle".into();
    signs.language = Some("eng".into());
    signs.forced = true;
    signs.subtitle = Some(SubtitleProbe { text: false });
    let probe = Probe {
        source: "/films/film.mkv".into(),
        format: "Matroska / WebM".into(),
        duration: Some(5025.5),
        size: Some(1_234_567_890),
        bit_rate: Some(12_340_000),
        tags: BTreeMap::from([
            ("title".into(), "Film".into()),
            ("encoder".into(), "Lavf".into()),
        ]),
        streams: vec![film, signs],
        chapters: vec![
            Chapter {
                start: 0.0,
                end: 90.25,
                title: Some("Opening".into()),
            },
            Chapter {
                start: 90.25,
                end: 5025.5,
                title: None,
            },
        ],
        subtitle_files: vec![SubtitleFile {
            path: "/films/film.fr.srt".into(),
            codec: "subrip".into(),
            language: Some("fre".into()),
            title: Some("French".into()),
            forced: true,
            hearing_impaired: true,
        }],
    };
    assert_eq!(
        text(&probe),
        "/films/film.mkv\n\
         Format     Matroska / WebM\n\
         Duration   1:23:45.500\n\
         Size       1.2 GB\n\
         Bit rate   12.3 Mb/s\n\
         Tags       encoder: Lavf\n\
         \x20          title: Film\n\
         Streams     0  Video       HEVC Main 10 · 3840×2160 · 23.976 fps · yuv420p10le · \
         bt2020nc/bt2020/smpte2084, limited range · HDR10 · 20 Mb/s · Default\n\
         \x20          10  Subtitles   PGS · pictures · English · Forced\n\
         Chapters   1  0:00:00.000–0:01:30.250  Opening\n\
         \x20          2  0:01:30.250–1:23:45.500  Chapter 2\n\
         Beside it  /films/film.fr.srt · SRT · French · Forced · SDH\n"
    );

    let mut timecode = bare(Kind::Data);
    timecode.codec_tag = Some("tmcd".into());
    let live = Probe {
        source: "https://example.com/live.m3u8".into(),
        format: "Apple HTTP Live Streaming".into(),
        duration: None,
        size: None,
        bit_rate: None,
        tags: BTreeMap::new(),
        streams: vec![timecode],
        chapters: Vec::new(),
        subtitle_files: Vec::new(),
    };
    assert_eq!(
        text(&live),
        "https://example.com/live.m3u8\n\
         Format   Apple HTTP Live Streaming\n\
         Streams  0  Data        tmcd\n",
        "lined up after the longest heading it has"
    );
}

#[test]
fn a_stream_says_what_it_holds_and_what_it_is() {
    for (kind, cover, shown) in [
        (Kind::Video, false, "Video"),
        (Kind::Video, true, "Cover"),
        (Kind::Audio, false, "Audio"),
        (Kind::Subtitle, false, "Subtitles"),
        (Kind::Attachment, false, "Attachment"),
        (Kind::Data, false, "Data"),
    ] {
        let stream = StreamProbe {
            cover,
            ..bare(kind)
        };
        assert_eq!(stream_line(&stream, 1), format!("0  {shown:<10}  Unknown"));
    }
    let mut numbered = bare(Kind::Data);
    numbered.index = 3;
    assert_eq!(stream_line(&numbered, 3), "  3  Data        Unknown");

    let mut dts = bare(Kind::Audio);
    dts.codec = "dts".into();
    dts.profile = Some("DTS".into());
    assert_eq!(
        stream_line(&dts, 1),
        "0  Audio       DTS",
        "its profile only its name again"
    );
    dts.profile = Some("DTS-HD MA".into());
    dts.audio = Some(AudioProbe {
        sample_rate: 44_100,
        channels: 6,
        sample_format: Some("s32p"),
        bits: Some(24),
        ..sound()
    });
    dts.bit_rate = Some(1_509_000);
    assert_eq!(
        stream_line(&dts, 1),
        "0  Audio       DTS DTS-HD MA · 44.1 kHz · 6 channels · s32p · 24-bit · 1.5 Mb/s"
    );
    dts.audio = Some(AudioProbe {
        layout: Some("5.1(side)".into()),
        channels: 6,
        ..sound()
    });
    assert_eq!(
        stream_line(&dts, 1),
        "0  Audio       DTS DTS-HD MA · 5.1(side) · 1.5 Mb/s"
    );
    dts.audio = Some(sound());
    assert_eq!(
        stream_line(&dts, 1),
        "0  Audio       DTS DTS-HD MA · 1.5 Mb/s"
    );

    let mut cues = bare(Kind::Subtitle);
    cues.codec = "ass".into();
    cues.subtitle = Some(SubtitleProbe { text: true });
    cues.language = Some("jpn".into());
    cues.title = Some("Signs".into());
    cues.hearing_impaired = true;
    assert_eq!(
        stream_line(&cues, 1),
        "0  Subtitles   ASS · text · Japanese · “Signs” · SDH"
    );

    let mut film = bare(Kind::Video);
    film.codec = "av1".into();
    film.video = Some(pictures(1920, 1080));
    assert_eq!(stream_line(&film, 1), "0  Video       AV1 · 1920×1080");
}

#[test]
fn pictures_say_their_shape_turn_rate_pixels_and_colours() {
    assert_eq!(video_parts(&pictures(320, 240)), ["320×240"]);
    assert_eq!(
        shown(VideoProbe {
            pixel_aspect: Some("4:3".into()),
            display_width: 427,
            frame_rate: Some(25.0),
            first_field: Some("bottom"),
            pixel_format: Some("yuv420p".into()),
            ..pictures(320, 240)
        }),
        [
            "4:3 pixels, shown 427×240",
            "25 fps",
            "interlaced, bottom field first",
            "yuv420p"
        ]
    );
    for (rotation, mirrored, turned) in [
        (0, true, vec!["mirrored"]),
        (270, false, vec!["turned 270°"]),
        (90, true, vec!["mirrored, turned 90°"]),
    ] {
        assert_eq!(
            shown(VideoProbe {
                rotation,
                mirrored,
                ..pictures(320, 240)
            }),
            turned
        );
    }
    let colours = |matrix, primaries, transfer, range| {
        shown(VideoProbe {
            matrix,
            primaries,
            transfer,
            range,
            ..pictures(320, 240)
        })
    };
    assert_eq!(colours(None, None, None, Some("full")), ["full range"]);
    assert_eq!(colours(Some("bt709"), None, None, None), ["bt709"]);
    assert_eq!(
        colours(None, Some("bt709"), Some("bt709"), None),
        ["bt709/bt709"]
    );
    assert_eq!(
        colours(
            Some("bt470bg"),
            Some("smpte170m"),
            Some("bt709"),
            Some("limited")
        ),
        ["bt470bg/smpte170m/bt709, limited range"]
    );
}

#[test]
fn hdr_pictures_say_their_dolby_vision_and_light() {
    let dolby_vision = |name: &str, base_layer, enhancement_layer| {
        let dolby_vision = DolbyVision {
            name: name.into(),
            profile: 0,
            level: 0,
            compatibility: 0,
            base_layer,
            enhancement_layer,
        };
        shown(VideoProbe {
            dolby_vision: Some(dolby_vision),
            ..pictures(320, 240)
        })
    };
    assert_eq!(
        dolby_vision("7", true, true),
        ["Dolby Vision 7 with its enhancement layer"]
    );
    assert_eq!(
        dolby_vision("7", false, true),
        ["Dolby Vision 7 enhancement layer"]
    );
    assert_eq!(dolby_vision("8.1", true, false), ["Dolby Vision 8.1"]);
    assert_eq!(dolby_vision("5", false, false), ["Dolby Vision 5"]);

    let light = |mastering_peak, max_cll, max_fall| {
        shown(VideoProbe {
            mastering_peak,
            max_cll,
            max_fall,
            hdr: Some("HLG"),
            ..pictures(320, 240)
        })
    };
    assert_eq!(light(None, None, None), ["HLG"]);
    assert_eq!(
        light(Some(1000.0), Some(1000), Some(400)),
        [
            "HLG",
            "mastered at 1000 nits",
            "MaxCLL 1000 nits, MaxFALL 400 nits"
        ]
    );
    assert_eq!(
        light(Some(0.05), None, Some(400)),
        ["HLG", "mastered at 0.1 nits", "MaxFALL 400 nits"]
    );
    assert_eq!(light(None, Some(2010), None), ["HLG", "MaxCLL 2010 nits"]);
}

#[test]
fn a_track_says_its_language_its_title_unless_its_the_same_and_its_marks() {
    let some = |text: &str| Some(text.to_owned());
    assert_eq!(
        flags(&None, &None, false, false, false),
        Vec::<String>::new()
    );
    assert_eq!(
        flags(&some("fre"), &some("french"), false, false, false),
        ["French"]
    );
    assert_eq!(
        flags(&some("fre"), &some("Commentary"), true, false, false),
        ["French", "“Commentary”", "Default"]
    );
    assert_eq!(
        flags(&None, &some("English"), false, true, true),
        ["“English”", "Forced", "SDH"]
    );
    assert_eq!(
        flags(&None, &None, true, true, true),
        ["Default", "Forced", "SDH"]
    );
}

#[test]
fn times_sizes_and_rates_are_said_as_people_read_them() {
    for (seconds, shown) in [
        (0.0, "0:00:00.000"),
        (-1.0, "0:00:00.000"),
        (59.999, "0:00:59.999"),
        (90.5, "0:01:30.500"),
        (3599.9996, "1:00:00.000"),
        (36_061.001, "10:01:01.001"),
    ] {
        assert_eq!(time(seconds), shown, "{seconds}");
    }
    for (size, shown) in [
        (0, "0 bytes"),
        (999, "999 bytes"),
        (1000, "1 kB"),
        (47_520, "47.5 kB"),
        (999_949, "999.9 kB"),
        (1_000_000, "1 MB"),
        (1_260_000, "1.3 MB"),
        (1_000_000_000, "1 GB"),
        (2_540_000_000_000, "2540 GB"),
    ] {
        assert_eq!(bytes(size), shown, "{size}");
    }
    for (rate, shown) in [
        (0, "0 b/s"),
        (999, "999 b/s"),
        (1000, "1 kb/s"),
        (190_400, "190 kb/s"),
        (999_499, "999 kb/s"),
        (1_000_000, "1 Mb/s"),
        (12_340_000, "12.3 Mb/s"),
    ] {
        assert_eq!(bit_rate(rate), shown, "{rate}");
    }
    for (value, places, shown) in [
        (25.0, 3, "25"),
        (100.0, 0, "100"),
        (10.0, 1, "10"),
        (23.976_023, 3, "23.976"),
        (44.1, 3, "44.1"),
        (0.04, 1, "0"),
    ] {
        assert_eq!(number(value, places), shown, "{value} to {places}");
    }
}
