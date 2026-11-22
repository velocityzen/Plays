use super::*;

#[test]
fn media_with_subtitles_as_load_takes_it() {
    let media = Media {
        url: "http://192.0.2.1:8080/a/master.m3u8".into(),
        content_type: "application/x-mpegURL".into(),
        stream_type: StreamType::Buffered,
        title: Some("A Film".into()),
        duration: Some(5400.0),
        text_tracks: vec![TextTrack {
            id: 1,
            url: "http://192.0.2.1:8080/a/subs.vtt".into(),
            content_type: "text/vtt".into(),
            name: "English".into(),
            language: Some("en".into()),
        }],
        hls_segments: Some(HlsSegments::Fmp4),
    };
    let style = TextTrackStyle {
        font_scale: 1.25,
        italic: true,
        ..TextTrackStyle::default()
    };
    let json = media.json(Some(&style));
    assert_eq!(json["contentId"], media.url);
    assert_eq!(json["streamType"], "BUFFERED");
    assert_eq!(json["metadata"]["title"], "A Film");
    assert_eq!(json["duration"], 5400.0);
    assert_eq!(json["hlsSegmentFormat"], "fmp4");
    assert_eq!(json["hlsVideoSegmentFormat"], "fmp4");
    assert_eq!(
        json["tracks"][0]["trackContentId"],
        "http://192.0.2.1:8080/a/subs.vtt"
    );
    assert_eq!(json["tracks"][0]["language"], "en");
    let style = &json["textTrackStyle"];
    assert_eq!(style["foregroundColor"], "#FFFFFFFF");
    assert_eq!(style["backgroundColor"], "#00000000");
    assert_eq!(style["edgeType"], "OUTLINE");
    assert_eq!(style["fontScale"], 1.25);
    assert_eq!(style["fontStyle"], "ITALIC");
    assert!(style.get("fontFamily").is_none());
}

#[test]
fn reads_what_the_device_says_it_is_doing() {
    let status = json!({
        "mediaSessionId": 3,
        "playerState": "PLAYING",
        "currentTime": 61.5,
        "activeTrackIds": [1],
        "media": {"duration": 5400.0},
    });
    assert_eq!(
        MediaStatus::read(&status),
        Some(MediaStatus {
            state: PlayerState::Playing,
            position: 61.5,
            duration: Some(5400.0),
            active_tracks: vec![1],
            media_session: 3,
        })
    );
    let finished = json!({"mediaSessionId": 3, "playerState": "IDLE", "idleReason": "FINISHED"});
    assert_eq!(
        MediaStatus::read(&finished).unwrap().state,
        PlayerState::Idle(Some(IdleReason::Finished))
    );
    assert_eq!(MediaStatus::read(&json!({"playerState": "PLAYING"})), None);
}
