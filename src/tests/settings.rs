use gpui::TestAppContext;

use super::*;

/// A settings file of a test's own, holding `text`.
fn file(name: &str, text: &str) -> PathBuf {
    let path = files::test_folder(&format!("settings-{name}")).join("settings.json");
    files::write_whole(&path, text.as_bytes()).unwrap();
    path
}

/// What was kept of a settings file read with something wrong in it.
fn kept_aside(path: &Path) -> Option<String> {
    fs::read_to_string(files::with_suffix(path, ".bad")).ok()
}

#[test]
fn a_bad_value_costs_only_itself() {
    let text = r#"{
            "master_volume": 0.5,
            "theme": "purple",
            "subtitles": { "color": "red", "size": 1.5 },
            "hotkeys": { "play_pause": 7, "loop": ["cmd-l"] }
        }"#;
    let path = file("bad-value", text);
    let settings = Settings::load_from(&path);
    let default = Settings::default();
    assert_eq!(settings.master_volume, 0.5);
    assert_eq!(settings.theme, default.theme);
    assert_eq!(settings.subtitles.color, default.subtitles.color);
    assert_eq!(settings.subtitles.size, 1.5);
    assert_eq!(
        settings.hotkeys.keys("block.playback.play_pause"),
        default.hotkeys.keys("block.playback.play_pause")
    );
    assert_eq!(
        settings.hotkeys.keys("block.playback.loop.toggle"),
        ["cmd-l"]
    );
    assert_eq!(kept_aside(&path).as_deref(), Some(text), "the file kept");
    let Ok(Value::Object(fields)) = serde_json::from_str(text) else {
        panic!("an object");
    };
    let (_, mut unread) = leniently::<Settings>(fields);
    unread.sort();
    assert_eq!(unread, ["hotkeys.play_pause", "subtitles.color", "theme"]);
}

#[test]
fn a_file_half_written_is_kept_aside() {
    let text = r#"{ "master_volume": 0.5, "the"#;
    let path = file("half", text);
    assert_eq!(Settings::load_from(&path), Settings::default());
    assert_eq!(kept_aside(&path).as_deref(), Some(text));
}

#[test]
fn a_file_from_a_later_plays_is_kept_aside() {
    let text = r#"{ "version": 99, "master_volume": 0.5 }"#;
    let path = file("later", text);
    assert_eq!(Settings::load_from(&path).master_volume, 0.5);
    assert_eq!(kept_aside(&path).as_deref(), Some(text));
}

#[test]
fn a_good_file_is_read_as_it_is() {
    let settings = Settings {
        master_volume: 0.25,
        ..Settings::default()
    };
    let json = settings.to_json();
    assert!(
        json.starts_with(&format!("{{\n  \"version\": {VERSION},")),
        "{json}"
    );
    let path = file("good", &json);
    assert_eq!(Settings::load_from(&path), settings);
    assert_eq!(kept_aside(&path), None);
    // As are ones from before there was a version.
    let path = file("first", r#"{ "master_volume": 0.25 }"#);
    assert_eq!(Settings::load_from(&path), settings);
    assert_eq!(kept_aside(&path), None);
}

#[test]
fn exported_settings_import_as_they_were() {
    let settings = Settings {
        master_volume: 0.25,
        theme: ThemeMode::Dark,
        ..Settings::default()
    };
    let imported = Settings::import(&settings.to_json()).unwrap();
    assert_eq!(imported, settings);
}

#[test]
fn imports_take_the_defaults_for_what_they_leave_out() {
    let imported = Settings::import(r#"{ "master_volume": 0.5 }"#).unwrap();
    assert_eq!(imported.master_volume, 0.5);
    assert_eq!(imported.theme, Settings::default().theme);
}

#[test]
fn volumes_in_the_file_are_read_as_full_at_most() {
    let imported = Settings::import(r#"{ "default_volume": 3.0, "master_volume": 5.0 }"#).unwrap();
    assert_eq!(
        (imported.default_volume(), imported.master_volume()),
        (1.0, 1.0)
    );
    let path = file(
        "volumes",
        r#"{ "default_volume": -0.5, "master_volume": -2.0 }"#,
    );
    let loaded = Settings::load_from(&path);
    assert_eq!(
        (loaded.default_volume(), loaded.master_volume()),
        (0.0, 0.0)
    );
    let within = Settings::import(r#"{ "default_volume": 0.3, "master_volume": 0.6 }"#).unwrap();
    assert_eq!(
        (within.default_volume(), within.master_volume()),
        (0.3, 0.6)
    );
}

/// A number too big for its setting is left out: read as infinity, it
/// would be saved as nothing, and the file set aside the next time.
#[test]
fn a_number_too_big_for_its_setting_is_left_as_it_starts_out() {
    let imported = Settings::import(r#"{ "hdr_gamma": 1e300, "master_volume": 0.5 }"#).unwrap();
    assert_eq!(imported.hdr_gamma, Settings::default().hdr_gamma);
    assert_eq!(imported.master_volume, 0.5);
}

/// A setting this Plays can't take, as from a later one, leaves just that
/// one as it starts out, as loading does.
#[test]
fn imports_take_what_they_can_read() {
    let imported = Settings::import(
        r#"{ "version": 99, "master_volume": 0.5, "theme": "purple", "subtitles": { "color": "red", "size": 1.5 } }"#,
    )
    .unwrap();
    let default = Settings::default();
    assert_eq!(imported.master_volume, 0.5);
    assert_eq!(imported.theme, default.theme);
    assert_eq!(imported.subtitles.color, default.subtitles.color);
    assert_eq!(imported.subtitles.size, 1.5);
}

#[test]
fn other_files_dont_import() {
    assert!(Settings::import("not json").is_err());
    assert!(Settings::import("[1, 2]").is_err());
    assert!(Settings::import(r#"{ "name": "package", "version": "1.0" }"#).is_err());
}

#[test]
fn missing_fields_fall_back_to_defaults() {
    let settings: Settings =
        serde_json::from_str(r#"{ "master_volume": 0.5, "theme": "light" }"#).unwrap();
    assert_eq!(settings.master_volume, 0.5);
    assert_eq!(settings.theme, ThemeMode::Light);
    assert_eq!(settings.max_resolution, Settings::default().max_resolution);
}

#[test]
fn the_screensaver_starts_after_five_minutes_and_a_minute_at_least() {
    let settings = Settings::default();
    assert!(settings.screensaver);
    assert_eq!(settings.screensaver_after(), Duration::from_secs(5 * 60));
    let read = |minutes: u32| Settings {
        screensaver_minutes: minutes,
        ..Settings::default()
    };
    assert_eq!(read(30).screensaver_after(), Duration::from_secs(30 * 60));
    assert_eq!(read(0).screensaver_after(), Duration::from_secs(60));
}

#[test]
fn reads_the_audio_output() {
    let settings = Settings::default();
    assert_eq!(settings.audio_output.device, None);
    assert!(settings.audio_output.wait_for_device);
    let settings: Settings = serde_json::from_str(
        r#"{ "audio_output": { "device": { "id": "coreaudio:abc", "name": "DAC" } } }"#,
    )
    .unwrap();
    let device = settings.audio_output.device.as_ref().unwrap();
    assert_eq!(
        (device.id.as_str(), device.name.as_str()),
        ("coreaudio:abc", "DAC")
    );
    assert!(settings.audio_output.wait_for_device);
}

#[test]
fn every_protocol_looks_but_those_turned_off() {
    let settings = Settings::default();
    assert!(settings.casting.looks_with("DLNA"));
    let settings: Settings =
        serde_json::from_str(r#"{ "casting": { "look_with": { "DLNA": false } } }"#).unwrap();
    assert!(!settings.casting.looks_with("DLNA"));
    assert!(settings.casting.looks_with("Chromecast"));
}

#[test]
fn reads_and_writes_subtitle_styles() {
    let settings: Settings = serde_json::from_str(
        r##"{ "subtitles": { "color": "#FFD60A", "size": 9, "edge": "shadow" } }"##,
    )
    .unwrap();
    let style = settings.subtitles.clamped();
    assert_eq!(style.color, Rgb(0xffd60a));
    assert_eq!(style.size, 2.0);
    assert_eq!(style.edge, SubtitleEdge::Shadow);
    assert_eq!(style.weight, SubtitleWeight::Medium);
    let json = serde_json::to_string(&settings).unwrap();
    assert!(json.contains(r##""color":"#ffd60a""##), "{json}");
    assert!(serde_json::from_str::<Settings>(r#"{ "subtitles": { "color": "red" } }"#).is_err());
}

#[test]
fn reads_screenshot_settings() {
    let settings: Settings = serde_json::from_str(
        r#"{ "screenshots": { "folder": "/tmp/shots", "format": "jpeg", "jpeg_quality": 7 } }"#,
    )
    .unwrap();
    let shots = &settings.screenshots;
    assert_eq!(shots.folder(), Some(PathBuf::from("/tmp/shots")));
    assert_eq!(shots.format.extension(), "jpg");
    assert_eq!(shots.jpeg_quality(), 50);
    assert!(shots.subtitles);
    // HDR is kept only in PNGs.
    let keep = ScreenshotSettings {
        hdr: ScreenshotHdr::Keep,
        ..shots.clone()
    };
    assert!(!keep.keeps_hdr());
    assert!(
        ScreenshotSettings {
            format: ScreenshotFormat::Png,
            ..keep
        }
        .keeps_hdr()
    );
}

#[test]
fn picks_columns() {
    let auto = Settings::default();
    assert_eq!(auto.columns_for(1), 1);
    assert_eq!(auto.columns_for(2), 2);
    assert_eq!(auto.columns_for(4), 2);
    assert_eq!(auto.columns_for(5), 3);
    let fixed = Settings {
        columns: 4,
        ..Settings::default()
    };
    assert_eq!(fixed.columns_for(2), 4);
}

#[gpui::test]
fn tests_keep_no_settings_without_a_folder_of_their_own(cx: &mut TestAppContext) {
    assert_eq!(Folder::app(), Folder::default());
    assert_eq!(Settings::load(&Folder::app()), Settings::default());
    cx.update(|cx| assert_eq!(Settings::path(cx), None));
}

#[gpui::test]
fn settings_changed_are_saved_in_the_apps_folder_and_read_back(cx: &mut TestAppContext) {
    let folder = Folder::for_test("settings-saved");
    let path = folder.file(FILE).unwrap();
    cx.update(|cx| {
        cx.set_global(folder.clone());
        cx.set_global(Settings::default());
        assert_eq!(Settings::path(cx).as_ref(), Some(&path));
        Settings::update(cx, false, |settings| settings.master_volume = 0.5);
        assert!(!path.exists(), "not saved unless asked");
        Settings::update(cx, true, |settings| settings.resume_files = false);
        let saved = Settings::load(&folder);
        assert_eq!(&saved, cx.global::<Settings>());
        assert_eq!((saved.master_volume, saved.resume_files), (0.5, false));
    });
}

#[gpui::test]
fn a_broken_file_is_kept_aside_and_saved_over_whole(cx: &mut TestAppContext) {
    let folder = Folder::for_test("settings-broken");
    let path = folder.file(FILE).unwrap();
    let half = r#"{ "master_volume": 0.5, "the"#;
    files::write_whole(&path, half.as_bytes()).unwrap();
    let settings = Settings::load(&folder);
    assert_eq!(settings, Settings::default());
    cx.update(|cx| {
        cx.set_global(folder.clone());
        cx.set_global(settings);
        Settings::update(cx, true, |settings| settings.master_volume = 0.75);
    });
    assert_eq!(Settings::load(&folder).master_volume, 0.75);
    assert_eq!(kept_aside(&path).as_deref(), Some(half));
}

#[test]
fn panels_shown_by_name_are_those_named_whatever_the_order() {
    for bits in 0..16_u8 {
        let over_video = OverVideo {
            controls: bits & 1 != 0,
            top_bar: bits & 2 != 0,
            track_switcher: bits & 4 != 0,
            notes: bits & 8 != 0,
        };
        let mut visible = over_video.visible();
        assert_eq!(OverVideo::showing(&visible), over_video, "{visible:?}");
        visible.reverse();
        assert_eq!(OverVideo::showing(&visible), over_video, "{visible:?}");
    }
    assert_eq!(
        OverVideo::showing(&["notes", "bars"]),
        OverVideo {
            notes: true,
            ..OverVideo::NONE
        },
        "what it doesn't know passed over"
    );
}
