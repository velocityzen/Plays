use super::*;

#[test]
fn names_avc_from_either_form() {
    assert_eq!(
        avc(&[1, 0x64, 0x00, 0x28, 0xff]).as_deref(),
        Some("avc1.640028")
    );
    let annex_b = [
        0, 0, 0, 1, 0x67, 0x4d, 0x40, 0x1f, 0xaa, 0, 0, 1, 0x68, 0xce,
    ];
    assert_eq!(avc(&annex_b).as_deref(), Some("avc1.4d401f"));
    assert_eq!(avc(&[]), None);
}

#[test]
fn names_hevc() {
    // Main profile, main tier, level 4.1, progressive and frame-only.
    let mut hvcc = vec![1, 0x01, 0x60, 0, 0, 0, 0x90, 0, 0, 0, 0, 0, 123];
    hvcc.extend([0; 10]);
    assert_eq!(hevc(&hvcc).as_deref(), Some("hvc1.1.6.L123.90"));
    // Main 10, high tier, level 5.1, from a parameter set with an
    // emulation prevention byte among the compatibility flags.
    let sps = [
        0, 0, 1, 0x42, 0x01, 0x01, 0x22, 0x20, 0x00, 0x00, 0x03, 0x00, 0xb0, 0, 0, 0, 0, 0, 153,
    ];
    assert_eq!(hevc(&sps).as_deref(), Some("hvc1.2.4.H153.B0"));
}

#[test]
fn writes_a_master_playlist() {
    let variant = Variant {
        bandwidth: 2_000_000,
        codecs: Some("avc1.640028,mp4a.40.2".into()),
        resolution: Some((1920, 1080)),
        frame_rate: Some(25.0),
    };
    let subtitles = SubtitlesRendition {
        language: Some("en".into()),
        name: "English".into(),
    };
    assert_eq!(
        master(&variant, Some(&subtitles)),
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n\
             #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English\",LANGUAGE=\"en\",\
             DEFAULT=YES,AUTOSELECT=YES,FORCED=NO,URI=\"subs.m3u8\"\n\
             #EXT-X-STREAM-INF:BANDWIDTH=2000000,CODECS=\"avc1.640028,mp4a.40.2\",\
             RESOLUTION=1920x1080,FRAME-RATE=25.000,SUBTITLES=\"subs\",CLOSED-CAPTIONS=NONE\n\
             video.m3u8\n"
    );
}

#[test]
fn lists_a_files_segments() {
    let timeline = Timeline {
        starts: vec![0.0, 6.0],
        end: 10.5,
        keyframes: None,
    };
    assert_eq!(
        vod(&timeline, Segments::Fmp4),
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n\
             #EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MAP:URI=\"init.mp4\"\n\
             #EXTINF:6.000,\n0.m4s\n#EXTINF:4.500,\n1.m4s\n#EXT-X-ENDLIST\n"
    );
}
