use std::path::PathBuf;

use serde_json::json;

use super::*;
use crate::settings::{AudioDevice, Rgb, SeveralFiles, Visualizer};

/// The keys of the values in `value` at `at`, going into each object
/// that has something in it.
fn leaves(value: &Value, at: &str, keys: &mut Vec<String>) {
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            for (key, value) in fields {
                let key = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                leaves(value, &key, keys);
            }
        }
        _ => keys.push(at.to_owned()),
    }
}

#[test]
fn every_setting_the_file_keeps_has_a_key() {
    let file = serde_json::to_value(Settings::default()).unwrap();
    let mut kept = Vec::new();
    leaves(&file, "", &mut kept);
    // Each protocol's is listed for the protocols there are.
    kept.retain(|key| key != LOOK_WITH);
    kept.sort();
    let mut keys: Vec<String> = all(&[]).into_iter().map(|f| f.key.into_owned()).collect();
    keys.sort();
    assert_eq!(keys, kept);

    let protocols = all(&["Chromecast", "DLNA"]);
    let looking: Vec<(&str, &str, &str)> = protocols[protocols.len() - 2..]
        .iter()
        .map(|field| (field.key.as_ref(), field.title.as_ref(), field.pane))
        .collect();
    assert_eq!(
        looking,
        [
            ("casting.look_with.Chromecast", "Chromecast", "Casting"),
            ("casting.look_with.DLNA", "DLNA", "Casting"),
        ]
    );
}

#[test]
fn takes_what_the_window_offers() {
    let settings = Settings::default();
    let fonts = ["Menlo".to_owned()];
    let set = |key: &str, value: Value| {
        let field = find(key, &["Chromecast"]).unwrap();
        with(&settings, &field, value, &fonts).unwrap()
    };
    assert!(!set("autoplay", json!(false)).autoplay);
    assert_eq!(set("theme", json!("dark")).theme, ThemeMode::Dark);
    assert_eq!(set("columns", json!(3)).columns, 3);
    assert_eq!(set("seek_step_seconds", json!(30)).seek_step_seconds, 30);
    assert_eq!(
        set("several_files", json!("playlist")).several_files,
        SeveralFiles::Playlist
    );
    assert_eq!(set("master_volume", json!(0.25)).master_volume, 0.25);
    assert_eq!(
        set("audio_mode.visualizer", json!("none"))
            .audio_mode
            .visualizer,
        Visualizer::None
    );
    assert!(
        !set("audio_mode.show_quality", json!(false))
            .audio_mode
            .show_quality
    );
    assert_eq!(set("subtitles.size", json!(1.5)).subtitles.size, 1.5);
    assert_eq!(
        set("screenshots.jpeg_quality", json!(70))
            .screenshots
            .jpeg_quality,
        70
    );
    assert_eq!(
        set("subtitles.color", json!("#ffd60a")).subtitles.color,
        Rgb(0xffd60a)
    );
    assert_eq!(
        set("subtitles.font", json!("Menlo")).subtitles.font,
        "Menlo"
    );
    assert_eq!(set("subtitles.font", json!("")).subtitles.font, "");
    let folder = std::env::temp_dir().join("plays-shots");
    assert_eq!(
        set("screenshots.folder", json!(folder)).screenshots.folder,
        Some(folder)
    );
    assert_eq!(
        set("audio_output.device", json!({"id": "desk", "name": "Desk"}))
            .audio_output
            .device,
        Some(AudioDevice {
            id: "desk".into(),
            name: "Desk".into(),
        })
    );
    let keys = set("hotkeys", json!({"playback.play_pause": ["cmd-p"]})).hotkeys;
    assert_eq!(keys.keys("playback.play_pause"), ["cmd-p"]);
    let looking = set("casting.look_with.Chromecast", json!(false)).casting;
    assert!(!looking.looks_with("Chromecast"));
    assert!(looking.looks_with("DLNA"), "the others left be");
    let unset = set("screenshots.folder", Value::Null);
    assert_eq!(unset.screenshots.folder, None::<PathBuf>);
    assert_eq!(
        set("languages.audio", json!(["Japanese", "en", " FRA ", "eng"]))
            .languages
            .audio,
        ["jpn", "eng", "fre"],
        "by name or code, kept by code, once each"
    );
    assert_eq!(
        set("languages.subtitles", json!(["de"]))
            .languages
            .subtitles,
        ["ger"]
    );
    assert!(
        set("languages.subtitles", json!([]))
            .languages
            .subtitles
            .is_empty()
    );
}

#[test]
fn turns_the_rest_away_saying_what_it_takes() {
    let settings = Settings::default();
    let fonts = ["Menlo".to_owned()];
    let set = |key: &str, value: Value| {
        let field = find(key, &[]).unwrap();
        with(&settings, &field, value, &fonts).unwrap_err()
    };
    assert_eq!(set("autoplay", json!("yes")), "autoplay is true or false");
    assert_eq!(
        set("theme", json!("purple")),
        r#"theme is one of "system", "dark", "light""#
    );
    assert_eq!(set("columns", json!(7)), "columns is one of 0, 1, 2, 3, 4");
    assert_eq!(
        set("seek_step_seconds", json!(7)),
        "seek_step_seconds is one of 5, 10, 30, 60"
    );
    assert_eq!(
        set("several_files", json!("shuffled")),
        r#"several_files is one of "together", "playlist""#
    );
    assert_eq!(
        set("subtitles.size", json!(3.0)),
        "subtitles.size is a number from 0.5 to 2"
    );
    assert_eq!(
        set("master_volume", json!("loud")),
        "master_volume is a number from 0 to 1"
    );
    assert!(
        set("subtitles.color", json!("#123456"))
            .starts_with(r##"subtitles.color is one of "#ffffff""##),
        "a color the window doesn't offer"
    );
    assert_eq!(
        set("subtitles.font", json!("Comic Sans")),
        r#"subtitles.font is a font this system has, or "" for the system's own"#
    );
    assert_eq!(
        set("screenshots.folder", json!("shots")),
        "screenshots.folder is a folder's full path, or null for Pictures"
    );
    assert_eq!(
        set("audio_output.device", json!("desk")),
        "audio_output.device is an output, {id, name}, or null for the system's default"
    );
    assert!(
        set("audio_output.device", json!({"id": "desk"}))
            .starts_with("audio_output.device can't be that: missing field `name`"),
        "an output with no name"
    );
    assert!(
        set("screenshots.jpeg_quality", json!(70.5))
            .starts_with("screenshots.jpeg_quality can't be that"),
        "a quality between whole numbers"
    );
    assert_eq!(
        set("hotkeys", json!(["cmd-p"])),
        "hotkeys is the shortcuts changed, by command"
    );
    let languages =
        r#"languages.audio is a list of languages by name or code, like ["English", "fr"]"#;
    for wrong in [
        json!("English"),
        json!(["English", "Elvish"]),
        json!([1]),
        json!(null),
    ] {
        assert_eq!(set("languages.audio", wrong.clone()), languages, "{wrong}");
    }
}

#[test]
fn reads_a_setting_by_its_key() {
    let mut settings = Settings::default();
    settings.subtitles.size = 1.5;
    assert_eq!(get(&settings, "subtitles.size"), json!(1.5));
    assert_eq!(get(&settings, "theme"), json!("system"));
    assert_eq!(get(&settings, "audio_output.device"), Value::Null);
    assert_eq!(
        get(&settings, "casting.look_with.Chromecast"),
        json!(true),
        "a protocol left out looks"
    );
    settings
        .casting
        .look_with
        .insert("Chromecast".into(), false);
    assert_eq!(get(&settings, "casting.look_with.Chromecast"), json!(false));
}

#[test]
fn tells_which_settings_differ_in_their_order() {
    let old = Settings::default();
    let mut new = old.clone();
    assert!(changed(&old, &new, &["Chromecast"]).is_empty());
    new.subtitles.size += 1.0;
    new.autoplay = !new.autoplay;
    new.casting.look_with.insert("Chromecast".into(), false);
    assert_eq!(
        changed(&old, &new, &["Chromecast"]),
        ["autoplay", "subtitles.size", "casting.look_with.Chromecast"]
    );
    assert_eq!(
        changed(&old, &new, &[]),
        ["autoplay", "subtitles.size"],
        "a protocol not looked with isn't a setting"
    );
}
