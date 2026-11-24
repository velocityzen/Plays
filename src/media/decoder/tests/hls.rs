use url::Url;

use super::{attributes, data_url, is_playlist, one_variant};

const BASE: &str = "https://example.com/show/master.m3u8";

/// The master playlist `text` cut down, from `BASE`.
fn cut(text: &str, max_short_side: Option<u32>) -> Option<String> {
    one_variant(text, &Url::parse(BASE).unwrap(), max_short_side)
}

/// A master playlist like a streaming service's: one picture in three
/// sizes, the middle one twice with sound of two qualities, and a variant
/// with sound alone.
const MASTER: &str = r#"#EXTM3U
#EXT-X-INDEPENDENT-SEGMENTS
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="low",NAME="English",LANGUAGE="en",URI="low/en.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="high",NAME="English",LANGUAGE="en",DEFAULT=YES,URI="high/en.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="high",NAME="Español",LANGUAGE="es",URI="/elsewhere/es.m3u8"
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="English",LANGUAGE="en",URI="https://cdn.example.net/subs/en.m3u8"
#EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID="cc",NAME="English",INSTREAM-ID="CC1"

#EXT-X-STREAM-INF:BANDWIDTH=500000,CODECS="avc1.64001f,mp4a.40.5",RESOLUTION=640x360,AUDIO="low",SUBTITLES="subs",CLOSED-CAPTIONS="cc"
360/index.m3u8
#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=200000,RESOLUTION=640x360,URI="360/iframes.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=2000000,CODECS="avc1.64001f,mp4a.40.5",RESOLUTION=1280x720,AUDIO="low",SUBTITLES="subs",CLOSED-CAPTIONS="cc"
720/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2200000,CODECS="avc1.64001f,mp4a.40.2",RESOLUTION=1280x720,AUDIO="high",SUBTITLES="subs",CLOSED-CAPTIONS="cc"
720/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=6000000,CODECS="avc1.640028,mp4a.40.2",RESOLUTION=1920x1080,AUDIO="high",SUBTITLES="subs",CLOSED-CAPTIONS="cc"
../1080/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=9000000,CODECS="mp4a.40.2"
sound/index.m3u8
"#;

#[test]
fn plays_the_variant_with_the_most_bandwidth_alone() {
    let cut = cut(MASTER, None).unwrap();
    assert_eq!(
        cut,
        r#"#EXTM3U
#EXT-X-INDEPENDENT-SEGMENTS
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="high",NAME="English",LANGUAGE="en",DEFAULT=YES,URI="https://example.com/show/high/en.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="high",NAME="Español",LANGUAGE="es",URI="https://example.com/elsewhere/es.m3u8"
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="English",LANGUAGE="en",URI="https://cdn.example.net/subs/en.m3u8"
#EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID="cc",NAME="English",INSTREAM-ID="CC1"

#EXT-X-STREAM-INF:BANDWIDTH=6000000,CODECS="avc1.640028,mp4a.40.2",RESOLUTION=1920x1080,AUDIO="high",SUBTITLES="subs",CLOSED-CAPTIONS="cc"
https://example.com/1080/index.m3u8
"#
    );
}

#[test]
fn plays_the_best_variant_within_the_resolution_limit() {
    let cut = cut(MASTER, Some(720)).unwrap();
    // Of the two 720p variants, the one with the better sound.
    assert!(
        cut.contains("BANDWIDTH=2200000,")
            && cut.contains("\nhttps://example.com/show/720/index.m3u8\n"),
        "{cut}"
    );
    assert!(
        !cut.contains("BANDWIDTH=2000000,") && !cut.contains("1080"),
        "{cut}"
    );
    assert!(
        cut.contains("high/en.m3u8") && !cut.contains("low/en.m3u8"),
        "{cut}"
    );
}

#[test]
fn plays_the_smallest_picture_when_none_is_within_the_limit() {
    let cut = cut(MASTER, Some(240)).unwrap();
    assert!(cut.contains("RESOLUTION=640x360,"), "{cut}");
    assert!(
        !cut.contains("1280x720") && !cut.contains("1920x1080"),
        "{cut}"
    );
}

#[test]
fn of_variants_alike_plays_the_first() {
    let master = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=640x360\n\
        first.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=640x360\n\
        second.m3u8\n";
    let cut = cut(master, None).unwrap();
    assert!(
        cut.contains("first.m3u8") && !cut.contains("second"),
        "{cut}"
    );
}

#[test]
fn plays_sound_alone_only_when_theres_nothing_else() {
    let sound = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS=\"mp4a.40.5\"\n\
        low.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=256000,CODECS=\"ec-3\"\n\
        high.m3u8\n";
    let high = cut(sound, None).unwrap();
    assert!(
        high.contains("high.m3u8") && !high.contains("low.m3u8"),
        "{high}"
    );

    // A variant naming neither its size nor its codecs may have a picture;
    // codecs are named in any case.
    let unsaid = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=900000,CODECS=\"MP4A.40.2\"\n\
        sound.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=800000\n\
        unsaid.m3u8\n";
    let picture = cut(unsaid, None).unwrap();
    assert!(
        picture.contains("unsaid.m3u8") && !picture.contains("sound.m3u8"),
        "{picture}"
    );
}

#[test]
fn a_variant_whose_codecs_or_size_say_so_has_a_picture() {
    let master = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=900000,CODECS=\"mp4a.40.2\"\n\
        sound.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=800000,CODECS=\"hvc1.2.4.L123.B0,mp4a.40.2\"\n\
        picture.m3u8\n";
    let picture = cut(master, None).unwrap();
    assert!(
        picture.contains("picture.m3u8") && !picture.contains("sound.m3u8"),
        "{picture}"
    );

    // One giving its size has a picture, whatever its codecs leave out.
    let sized = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=900000,CODECS=\"mp4a.40.2\"\n\
        sound.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"mp4a.40.2\"\n\
        sized.m3u8\n";
    let picture = cut(sized, None).unwrap();
    assert!(
        picture.contains("sized.m3u8") && !picture.contains("sound.m3u8"),
        "{picture}"
    );
}

#[test]
fn is_left_alone_unless_a_master_playlist_lists_more_than_one_variant() {
    let media = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\nsegment0.ts\n#EXT-X-ENDLIST\n";
    assert_eq!(cut(media, None), None);
    let one = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nonly.m3u8\n";
    assert_eq!(cut(one, None), None);
    assert_eq!(cut("", None), None);
    // Variants listed in what doesn't start as a playlist does.
    let headless = "#EXT-X-STREAM-INF:BANDWIDTH=1000\na.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000\nb.m3u8\n";
    assert_eq!(cut(headless, None), None);
    assert!(cut(&format!("#EXTM3U\n{headless}"), None).is_some());
    assert_eq!(
        cut(&format!("not a playlist\n#EXTM3U\n{headless}"), None),
        None
    );
}

#[test]
fn takes_a_playlist_written_with_a_byte_order_mark_and_crlf() {
    let master = "\u{feff}#EXTM3U\r\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000\r\n\
        low.m3u8\r\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000\r\n\
        high.m3u8\r\n";
    let cut = cut(master, None).unwrap();
    assert!(
        cut.contains("\nhttps://example.com/show/high.m3u8\n"),
        "{cut}"
    );
    assert!(!cut.contains("low.m3u8"), "{cut}");
}

#[test]
fn takes_a_variants_playlist_from_the_next_line_naming_one() {
    let master = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000\n\
        low.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000\n\
        \n\
        # a comment\n\
        high.m3u8\n";
    assert_eq!(
        cut(master, None).unwrap(),
        "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000\n\
        \n\
        # a comment\n\
        https://example.com/show/high.m3u8\n"
    );
}

#[test]
fn is_left_alone_when_it_cant_be_cut_as_it_is() {
    // Variables in its addresses.
    let defines = "#EXTM3U\n\
        #EXT-X-DEFINE:NAME=\"host\",VALUE=\"https://cdn.example.net\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000\n\
        {$host}/low.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000\n\
        {$host}/high.m3u8\n";
    assert_eq!(cut(defines, None), None);
    // A variant naming no playlist.
    let unnamed = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000\n\
        low.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000\n";
    assert_eq!(cut(unnamed, None), None);
    // The one to play somewhere other than HTTP.
    let local = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000\n\
        low.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000\n\
        file:///etc/high.m3u8\n";
    assert_eq!(cut(local, None), None);
    // Its sound somewhere other than HTTP.
    let sound = "#EXTM3U\n\
        #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"x\",URI=\"file:///etc/en.m3u8\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"a\"\n\
        low.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000,AUDIO=\"a\"\n\
        high.m3u8\n";
    assert_eq!(cut(sound, None), None);
}

#[test]
fn keeps_renditions_only_of_the_groups_the_variant_plays_with() {
    let master = "#EXTM3U\n\
        #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"one\",URI=\"a.m3u8\"\n\
        #EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID=\"a\",NAME=\"angle\",URI=\"angle.m3u8\"\n\
        #EXT-X-MEDIA:TYPE=AUDIO,NAME=\"no group\",URI=\"nogroup.m3u8\"\n\
        #EXT-X-MEDIA:GROUP-ID=\"a\",NAME=\"no type\",URI=\"notype.m3u8\"\n\
        #EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"b\"\n\
        low.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000,AUDIO=\"a\"\n\
        high.m3u8\n";
    let cut = cut(master, None).unwrap();
    assert!(cut.contains("show/a.m3u8"), "{cut}");
    // Of another type, though of a group of the same name.
    assert!(!cut.contains("angle"), "{cut}");
    assert!(!cut.contains("nogroup") && !cut.contains("notype"), "{cut}");
}

#[test]
fn reads_attributes_as_written() {
    assert_eq!(
        attributes(
            r#"BANDWIDTH=1000,CODECS="avc1.64001f,mp4a.40.2",RESOLUTION=640x360,NAME="a=b""#
        ),
        [
            ("BANDWIDTH", "1000"),
            ("CODECS", r#""avc1.64001f,mp4a.40.2""#),
            ("RESOLUTION", "640x360"),
            ("NAME", r#""a=b""#),
        ]
    );
    // A quote left open runs to the end.
    assert_eq!(
        attributes(r#"NAME="open,TYPE=AUDIO"#),
        [("NAME", r#""open,TYPE=AUDIO"#)]
    );
    assert!(attributes("").is_empty());
}

#[test]
fn reads_only_playlists_over_http() {
    let playlist = |url: &str| is_playlist(&Url::parse(url).unwrap());
    assert!(playlist("https://example.com/live/master.m3u8"));
    assert!(playlist("http://example.com/MASTER.M3U8?token=1"));
    assert!(playlist("https://example.com/list.m3u"));
    assert!(!playlist("https://example.com/movie.mp4"));
    assert!(!playlist("https://example.com/master.m3u8.mp4"));
    assert!(!playlist("rtsp://example.com/master.m3u8"));
}

#[test]
fn a_cut_playlist_is_named_for_ffmpeg_last() {
    let url = data_url("#EXTM3U\nhigh.m3u8\n");
    assert_eq!(
        url,
        "data:application/vnd.apple.mpegurl,#EXTM3U\nhigh.m3u8\n# .m3u8"
    );
}
