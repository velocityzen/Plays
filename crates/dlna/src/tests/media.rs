use super::*;
use crate::xml::Element;

#[test]
fn protocol_info_entries_and_their_profiles() {
    let mp4 =
        Capability::parse("http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD_AAC;DLNA.ORG_FLAGS=8d70")
            .unwrap();
    assert_eq!(mp4.protocol, "http-get");
    assert_eq!(mp4.mime, "video/mp4");
    assert_eq!(mp4.profile.as_deref(), Some("AVC_MP4_HP_HD_AAC"));
    assert!(mp4.is_http() && mp4.takes("VIDEO/MP4") && !mp4.takes("video/mpeg"));
    assert_eq!(
        mp4.to_string(),
        "http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD_AAC;DLNA.ORG_FLAGS=8d70"
    );
    let any = Capability::parse("x-rincon:*:*:*").unwrap();
    assert!(any.takes("audio/flac") && !any.is_http());
    assert_eq!(any.profile, None);
    assert_eq!(Capability::parse("http-get:*:video/mp4"), None);
    assert_eq!(Capability::parse(""), None);
    let list = Capability::parse_list(
        " http-get:*:audio/mpeg:DLNA.ORG_PN=MP3, ,http-get:*:x/odd:a=1\\,2,broken",
    );
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].info, "a=1,2");
}

#[test]
fn protocol_info_from_real_renderers() {
    let samsung = Capability::parse_list(include_str!("../../fixtures/samsung-protocol-info.txt"));
    assert_eq!(samsung.len(), 18);
    assert!(samsung.iter().all(Capability::is_http));
    assert!(
        samsung
            .iter()
            .any(|it| it.mime == "video/mpeg" && it.profile.as_deref() == Some("MPEG_TS_HD_NA"))
    );
    assert!(samsung.iter().any(|it| it.takes("text/srt")));
    let lg = Capability::parse_list(include_str!("../../fixtures/lg-protocol-info.txt"));
    assert_eq!(lg.len(), 11);
    assert_eq!(lg.iter().filter(|it| !it.is_http()).count(), 1);
    assert!(
        lg.iter()
            .any(|it| it.takes("video/x-matroska") && it.profile.is_none())
    );
    let devialet =
        Capability::parse_list(include_str!("../../fixtures/devialet-protocol-info.txt"));
    assert_eq!(devialet.len(), 57);
    assert!(
        devialet
            .iter()
            .all(|it| it.is_http() && it.profile.is_none())
    );
    assert!(
        devialet
            .iter()
            .any(|it| it.takes("audio/L16;rate=48000;channels=2"))
    );
    assert!(
        devialet
            .iter()
            .any(|it| it.mime == "audio/l16;channels=2;rate=44100")
    );
    assert!(!devialet.iter().any(|it| it.mime.starts_with("video/")));
    let sonos = Capability::parse_list(include_str!("../../fixtures/sonos-protocol-info.txt"));
    assert!(
        sonos
            .iter()
            .any(|it| it.is_http() && it.takes("audio/flac"))
    );
    assert!(
        !sonos
            .iter()
            .any(|it| it.is_http() && it.mime.starts_with("video/"))
    );
}

fn film() -> Media {
    Media {
        url: "http://192.0.2.1:8080/cast/media?x=1&y=2".into(),
        mime: "video/mpeg".into(),
        features: Some("DLNA.ORG_OP=10;DLNA.ORG_CI=1".into()),
        kind: Kind::Video,
        title: Some("Tom & Jerry".into()),
        duration: Some(5400.25),
        size: None,
        subtitles: Some(Subtitles {
            url: "http://192.0.2.1:8080/cast/subs.srt".into(),
            mime: "text/srt".into(),
        }),
    }
}

#[test]
fn didl_lite_with_subtitles_both_ways() {
    let didl = Element::parse(&film().didl()).unwrap();
    let item = didl.child("item").unwrap();
    assert_eq!(item.child_text("title"), Some("Tom & Jerry"));
    assert_eq!(item.child_text("class"), Some("object.item.videoItem"));
    let res: Vec<&Element> = item.children("res").collect();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].text(), "http://192.0.2.1:8080/cast/media?x=1&y=2");
    assert_eq!(
        res[0].attribute("protocolInfo"),
        Some("http-get:*:video/mpeg:DLNA.ORG_OP=10;DLNA.ORG_CI=1")
    );
    assert_eq!(res[0].attribute("duration"), Some("1:30:00.250"));
    assert_eq!(res[0].attribute("size"), None);
    assert_eq!(res[1].text(), "http://192.0.2.1:8080/cast/subs.srt");
    assert_eq!(
        res[1].attribute("protocolInfo"),
        Some("http-get:*:text/srt:*")
    );
    let caption = item.child("CaptionInfoEx").unwrap();
    assert_eq!(caption.text(), "http://192.0.2.1:8080/cast/subs.srt");
    assert_eq!(caption.attribute("type"), Some("srt"));
    assert!(item.child("CaptionInfo").is_some());
}

#[test]
fn didl_lite_without_subtitles_or_title() {
    let song = Media {
        url: "http://192.0.2.1:8080/music/song.flac?token=1".into(),
        mime: "audio/flac".into(),
        features: None,
        kind: Kind::Audio,
        title: None,
        duration: None,
        size: Some(123),
        subtitles: None,
    };
    let didl = Element::parse(&song.didl()).unwrap();
    let item = didl.child("item").unwrap();
    assert_eq!(item.child_text("title"), Some("song.flac"));
    assert_eq!(item.children("res").count(), 1);
    assert!(item.child("CaptionInfoEx").is_none());
    let res = item.child("res").unwrap();
    assert_eq!(
        res.attribute("protocolInfo"),
        Some("http-get:*:audio/flac:*")
    );
    assert_eq!(res.attribute("size"), Some("123"));
    assert_eq!(res.attribute("duration"), None);
}

#[test]
fn times_as_renderers_say_them() {
    assert_eq!(clock(3723.9), "1:02:03");
    assert_eq!(clock(-5.0), "0:00:00");
    assert_eq!(clock_exact(59.9996), "0:01:00.000");
    assert_eq!(clock_exact(61.5), "0:01:01.500");
    assert_eq!(seconds("1:02:03"), Some(3723.0));
    assert_eq!(seconds("00:00:01.500"), Some(1.5));
    assert_eq!(seconds("+10:00:00"), Some(36000.0));
    assert_eq!(seconds("NOT_IMPLEMENTED"), None);
    assert_eq!(seconds(""), None);
    assert_eq!(seconds("1:2:3:4"), None);
}

#[test]
fn transport_states() {
    assert_eq!(
        TransportState::parse("PAUSED_PLAYBACK"),
        TransportState::Paused
    );
    assert_eq!(TransportState::parse(" PLAYING "), TransportState::Playing);
    assert_eq!(
        TransportState::parse("NO_MEDIA_PRESENT"),
        TransportState::NoMedia
    );
    assert_eq!(
        TransportState::parse("RECORDING"),
        TransportState::Other("RECORDING".into())
    );
}
