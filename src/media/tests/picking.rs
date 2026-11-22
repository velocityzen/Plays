use super::*;

/// A track of `kind` at `index`, in `language`.
fn track(index: usize, kind: TrackKind, language: Option<&str>) -> Track {
    Track {
        index,
        kind,
        language: language.map(str::to_owned),
        title: None,
        codec: "aac".to_owned(),
        format: None,
        default: false,
        forced: false,
        hearing_impaired: false,
        file: None,
    }
}

fn audio(index: usize, language: &str) -> Track {
    track(index, TrackKind::Audio, Some(language))
}

fn subtitles(index: usize, language: &str) -> Track {
    track(index, TrackKind::Subtitle, Some(language))
}

fn default(mut track: Track) -> Track {
    track.default = true;
    track
}

fn forced(mut track: Track) -> Track {
    track.forced = true;
    track
}

fn beside(mut track: Track) -> Track {
    track.file = Some("Movie.srt".into());
    track
}

fn wanting(audio: &[&str], subtitles: &[&str]) -> Wanted {
    Wanted {
        picked: Picked::default(),
        audio_languages: audio.iter().map(|&l| l.to_owned()).collect(),
        subtitle_languages: subtitles.iter().map(|&l| l.to_owned()).collect(),
    }
}

#[test]
fn audio_plays_in_the_first_language_preferred_the_file_has() {
    let tracks = [
        track(0, TrackKind::Video, None),
        default(audio(1, "jpn")),
        audio(2, "eng"),
        audio(3, "fre"),
        subtitles(4, "eng"),
    ];
    assert_eq!(wanting(&[], &[]).audio_track(&tracks), None, "the file's");
    assert_eq!(wanting(&["eng"], &[]).audio_track(&tracks), Some(2));
    assert_eq!(
        wanting(&["ger", "fre", "eng"], &[]).audio_track(&tracks),
        Some(3)
    );
    assert_eq!(
        wanting(&["en"], &[]).audio_track(&tracks),
        Some(2),
        "any code"
    );
    assert_eq!(wanting(&["ger"], &[]).audio_track(&tracks), None);
    assert_eq!(
        wanting(&["eng"], &["fre"]).audio_track(&tracks),
        Some(2),
        "subtitles' languages are theirs"
    );
}

#[test]
fn audio_in_a_language_is_its_default_track_else_its_first() {
    let tracks = [
        audio(1, "eng"),
        default(audio(2, "eng")),
        audio(3, "fre"),
        audio(4, "fre"),
        track(5, TrackKind::Audio, None),
    ];
    assert_eq!(wanting(&["eng"], &[]).audio_track(&tracks), Some(2));
    assert_eq!(wanting(&["fre"], &[]).audio_track(&tracks), Some(3));
}

#[test]
fn audio_picked_in_the_file_plays_again_if_it_has_it() {
    let tracks = [
        track(0, TrackKind::Video, None),
        audio(1, "eng"),
        audio(2, "fre"),
        subtitles(3, "ger"),
    ];
    let mut wanted = wanting(&["eng"], &[]);
    wanted.picked.audio = Some(2);
    assert_eq!(wanted.audio_track(&tracks), Some(2), "over the language");
    for gone in [0, 3, 9] {
        wanted.picked.audio = Some(gone);
        assert_eq!(wanted.audio_track(&tracks), Some(1), "no audio {gone}");
    }
}

#[test]
fn subtitles_show_in_the_first_language_preferred_the_file_has() {
    let tracks = [
        default(audio(1, "eng")),
        default(subtitles(2, "eng")),
        subtitles(3, "fre"),
        subtitles(4, "ger"),
    ];
    assert_eq!(
        wanting(&[], &[]).subtitle_track(&tracks),
        Some(2),
        "the default"
    );
    assert_eq!(wanting(&[], &["fre"]).subtitle_track(&tracks), Some(3));
    assert_eq!(
        wanting(&[], &["ita", "de", "fr"]).subtitle_track(&tracks),
        Some(4)
    );
    assert_eq!(wanting(&[], &["ita"]).subtitle_track(&tracks), Some(2));
    assert_eq!(
        wanting(&["ger"], &[]).subtitle_track(&tracks),
        Some(2),
        "audio's languages are its own"
    );
}

#[test]
fn subtitles_in_a_language_are_a_file_beside_else_the_default_else_the_first_whole() {
    let tracks = [
        forced(subtitles(1, "eng")),
        subtitles(2, "eng"),
        default(subtitles(3, "eng")),
        beside(subtitles(4, "eng")),
        forced(subtitles(5, "fre")),
        subtitles(6, "ger"),
        subtitles(7, "ger"),
    ];
    let show = |language: &str, tracks: &[Track]| wanting(&[], &[language]).subtitle_track(tracks);
    assert_eq!(show("eng", &tracks), Some(4), "the file beside");
    assert_eq!(show("eng", &tracks[..3]), Some(3), "the default");
    assert_eq!(show("eng", &tracks[..2]), Some(2), "the first whole");
    assert_eq!(show("eng", &tracks[..1]), Some(1), "forced, if that's all");
    assert_eq!(show("fre", &tracks), Some(5), "forced, if that's all");
    assert_eq!(show("ger", &tracks), Some(6));
    let mut forced_beside = beside(subtitles(8, "ita"));
    forced_beside.forced = true;
    let tracks = [subtitles(1, "ita"), forced_beside];
    assert_eq!(show("ita", &tracks), Some(1), "whole first");
}

#[test]
fn subtitles_with_no_language_preferred_are_as_the_file_wants() {
    let none = wanting(&[], &[]);
    let show = |tracks: &[Track]| none.subtitle_track(tracks);
    let tracks = [
        subtitles(1, "eng"),
        forced(subtitles(2, "eng")),
        default(subtitles(3, "fre")),
        beside(subtitles(4, "ger")),
        default(beside(subtitles(5, "ita"))),
        forced(beside(subtitles(6, "spa"))),
    ];
    assert_eq!(show(&tracks), Some(5), "the default file beside");
    assert_eq!(show(&tracks[..5]), Some(5));
    assert_eq!(show(&tracks[..4]), Some(4), "a file beside");
    assert_eq!(show(&tracks[..3]), Some(3), "the default");
    assert_eq!(show(&tracks[..2]), Some(2), "a forced one");
    assert_eq!(show(&tracks[..1]), None, "none it wants");
    assert_eq!(show(&[forced(beside(subtitles(6, "spa")))]), Some(6));
    assert_eq!(show(&[]), None);
}

#[test]
fn subtitles_picked_in_the_file_show_again_or_stay_hidden() {
    let tracks = [
        audio(1, "eng"),
        default(subtitles(2, "eng")),
        subtitles(3, "fre"),
    ];
    let mut wanted = wanting(&[], &["eng"]);
    wanted.picked.subtitles = Some(Subtitles::Track(3));
    assert_eq!(wanted.subtitle_track(&tracks), Some(3), "over the language");
    wanted.picked.subtitles = Some(Subtitles::Off);
    assert_eq!(wanted.subtitle_track(&tracks), None, "hidden");
    for gone in [1, 9] {
        wanted.picked.subtitles = Some(Subtitles::Track(gone));
        assert_eq!(
            wanted.subtitle_track(&tracks),
            Some(2),
            "no subtitles {gone}"
        );
    }
}

#[test]
fn picking_notes_audio_and_subtitles_but_not_video() {
    let mut picked = Picked::default();
    assert!(picked.is_empty());
    picked.pick(TrackKind::Video, Some(0));
    assert!(picked.is_empty(), "video isn't kept");
    picked.pick(TrackKind::Audio, Some(2));
    assert_eq!(picked.audio, Some(2));
    assert!(!picked.is_empty());
    picked.pick(TrackKind::Audio, None);
    assert_eq!(picked.audio, Some(2), "audio can't be turned off");
    picked.pick(TrackKind::Subtitle, Some(4));
    assert_eq!(picked.subtitles, Some(Subtitles::Track(4)));
    picked.pick(TrackKind::Subtitle, None);
    assert_eq!(picked.subtitles, Some(Subtitles::Off));
    let subtitles_only = Picked {
        audio: None,
        subtitles: Some(Subtitles::Off),
    };
    assert!(!subtitles_only.is_empty());
}

#[test]
fn picks_are_kept_short() {
    let json = |picked: Picked| serde_json::to_string(&picked).unwrap();
    assert_eq!(json(Picked::default()), "{}");
    let picked = Picked {
        audio: Some(2),
        subtitles: Some(Subtitles::Track(4)),
    };
    assert_eq!(json(picked), r#"{"audio":2,"subtitles":{"track":4}}"#);
    let off = Picked {
        audio: None,
        subtitles: Some(Subtitles::Off),
    };
    assert_eq!(json(off), r#"{"subtitles":"off"}"#);
    for picked in [Picked::default(), picked, off] {
        let read: Picked = serde_json::from_str(&json(picked)).unwrap();
        assert_eq!(read, picked);
    }
}
