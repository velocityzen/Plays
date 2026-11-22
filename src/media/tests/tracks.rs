use super::*;

fn subtitle(language: Option<&str>, title: Option<&str>) -> Track {
    Track {
        index: 2,
        kind: TrackKind::Subtitle,
        language: language.map(str::to_owned),
        title: title.map(str::to_owned),
        codec: "subrip".to_owned(),
        format: None,
        default: false,
        forced: false,
        hearing_impaired: false,
        file: None,
    }
}

#[test]
fn labels_tracks() {
    assert_eq!(
        subtitle(Some("eng"), Some("British")).label(1),
        "English – British"
    );
    assert_eq!(subtitle(Some("ger"), None).label(1), "German");
    assert_eq!(subtitle(Some("fr"), Some("French")).label(1), "French");
    assert_eq!(subtitle(Some("xyz"), None).label(1), "xyz");
    assert_eq!(subtitle(None, None).label(3), "Track 3");

    let mut forced = subtitle(Some("eng"), Some("British | Forced"));
    forced.forced = true;
    assert_eq!(forced.label(1), "English – British | Forced");
    let mut forced = subtitle(Some("eng"), None);
    forced.forced = true;
    forced.hearing_impaired = true;
    assert_eq!(forced.label(1), "English (Forced) (SDH)");
}

#[test]
fn describes_formats() {
    let mut audio = subtitle(Some("eng"), None);
    audio.kind = TrackKind::Audio;
    audio.codec = "eac3".to_owned();
    audio.format = channels_name(6);
    assert_eq!(audio.detail(), "E-AC-3 · 5.1");
    assert_eq!(subtitle(None, None).detail(), "SRT");
    let mut external = subtitle(None, None);
    external.file = Some("Movie.srt".into());
    assert_eq!(external.detail(), "SRT · External");
}

#[test]
fn tells_text_subtitles_from_pictures() {
    let mut track = subtitle(None, None);
    assert!(track.is_text());
    track.codec = "ass".to_owned();
    assert!(track.is_text());
    track.codec = "hdmv_pgs_subtitle".to_owned();
    assert!(!track.is_text());
    track.codec = "dvd_subtitle".to_owned();
    assert!(!track.is_text());
}

#[test]
fn names_languages() {
    assert_eq!(language_name("eng"), "English");
    assert_eq!(language_name("pt-BR"), "Portuguese (BR)");
    assert_eq!(language_name("xx-YY"), "xx-YY");
    assert_eq!(language_code("EN").as_deref(), Some("en"));
    assert_eq!(language_code("English").as_deref(), Some("eng"));
    assert_eq!(language_code("british"), None);
}

#[test]
fn a_language_is_kept_by_its_first_code() {
    assert_eq!(language("en"), Some("eng"));
    assert_eq!(language("ENG"), Some("eng"));
    assert_eq!(language("english"), Some("eng"));
    assert_eq!(language("fra"), Some("fre"));
    assert_eq!(language("Norwegian Bokmål"), Some("nob"));
    assert_eq!(language("british"), None);
    assert_eq!(language("pt-BR"), None, "no regions");
}

#[test]
fn languages_are_offered_a_to_z_but_multiple() {
    let all = languages();
    assert_eq!(all.len(), LANGUAGES.len() - 1);
    assert_eq!(all.first(), Some(&("afr", "Afrikaans")));
    assert_eq!(all.last(), Some(&("wel", "Welsh")));
    assert!(all.windows(2).all(|pair| pair[0].1 < pair[1].1), "A to Z");
    assert!(!all.iter().any(|(code, _)| *code == "mul"));
}

#[test]
fn a_language_is_the_same_in_any_form_and_region() {
    assert!(same_language("eng", "en"));
    assert!(same_language("en-GB", "eng"));
    assert!(same_language("FRA", "fre"));
    assert!(same_language("pt_BR", "por"));
    assert!(!same_language("eng", "fre"));
    assert!(!same_language("chi", "yue"), "Cantonese is its own");
    assert!(same_language("xx", "xx-YY"), "unknown, by its code");
    assert!(!same_language("xx", "xy"));
}

#[test]
fn tags_languages_as_bcp_47() {
    assert_eq!(language_tag("eng"), "en");
    assert_eq!(language_tag("GER"), "de");
    assert_eq!(language_tag("pt-BR"), "pt-BR");
    assert_eq!(language_tag("por_BR"), "pt-BR");
    assert_eq!(language_tag("yue"), "yue", "no two letters for it");
    assert_eq!(language_tag("xxx"), "xxx");
}
