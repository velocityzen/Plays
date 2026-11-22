//! The `settings` commands, and the Settings window setting each setting
//! through them; and copying the version and debug info.

use std::fs;
use std::sync::Arc;

use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use serde_json::{Value, json};

use super::open_settings;
use crate::api::{self, Error, Invocation};
use crate::app::AudioEngine;
use crate::cast::Casting;
use crate::cast::fake::FakeTarget;
use crate::files::Folder;
use crate::settings::{Settings, ThemeMode};
use crate::ui::about;
use crate::ui::settings::ffmpeg_version;
use crate::ui::testing::{self, click, find};

fn run(cx: &mut VisualTestContext, command: &str, args: Value) -> Result<Value, Error> {
    let done = TestAppContext::update(cx, |cx| {
        api::run(Invocation::new(command).with_args(args), cx)
    });
    cx.run_until_parked();
    done
}

/// Keep the settings in a folder of the test's own, `name`.
fn keep_in(cx: &mut VisualTestContext, name: &str) -> Folder {
    let folder = Folder::for_test(name);
    cx.update(|_, cx| cx.set_global(folder.clone()));
    folder
}

/// What the settings file in `folder` holds, if there is one.
fn saved(folder: &Folder) -> Option<String> {
    fs::read_to_string(folder.file("settings.json")?).ok()
}

fn settings<T>(cx: &mut VisualTestContext, read: impl FnOnce(&Settings) -> T) -> T {
    cx.update(|_, cx| read(cx.global::<Settings>()))
}

/// `settings.set` of `key` to `value`, as a control sends it.
fn set(key: &str, value: Value) -> Invocation {
    Invocation::new("settings.set").with_args(json!({"key": key, "value": value}))
}

/// Cast with the fake protocol, whose setting is `casting.look_with.Fake`.
fn with_fake_protocol(cx: &mut VisualTestContext) {
    let fake = Arc::new(FakeTarget::default());
    cx.update(|_, cx| cx.set_global(Casting::local(vec![fake])));
}

fn recorded(cx: &mut VisualTestContext) -> Vec<Invocation> {
    cx.update(|_, cx| api::recorded(cx))
}

#[gpui::test]
fn a_setting_set_in_the_window_and_by_command_is_saved_alike(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let clicked = keep_in(&mut cx, "commands-clicked");
    cx.update(|_, cx| api::record(cx));
    let mut window = open_settings(&mut cx);
    click(&mut window, "pane-Playback");
    click(&mut window, "autoplay");
    assert_eq!(recorded(&mut window), [set("autoplay", json!(false))]);
    assert!(!settings(&mut window, |s| s.autoplay));
    let from_window = saved(&clicked).expect("saved");

    // The same, by command, into a file of its own.
    let typed = keep_in(&mut cx, "commands-typed");
    cx.update(|_, cx| Settings::update(cx, false, |s| s.autoplay = true));
    assert_eq!(
        run(
            &mut cx,
            "settings.set",
            json!({"key": "autoplay", "value": false})
        ),
        Ok(json!(false))
    );
    assert_eq!(saved(&typed), Some(from_window), "saved alike");
}

#[gpui::test]
fn each_kind_of_control_sets_its_setting_by_command(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    with_fake_protocol(&mut cx);
    cx.update(|_, cx| api::record(cx));
    let mut window = open_settings(&mut cx);

    click(&mut window, "pane-Appearance");
    click(&mut window, "theme-Dark");
    click(&mut window, "columns-2");
    assert_eq!(
        recorded(&mut window),
        [set("theme", json!("dark")), set("columns", json!(2))]
    );
    assert_eq!(
        settings(&mut window, |s| (s.theme, s.columns)),
        (ThemeMode::Dark, 2)
    );

    click(&mut window, "pane-Casting");
    click(&mut window, "Fake");
    assert_eq!(
        recorded(&mut window),
        [set("casting.look_with.Fake", json!(false))]
    );
    assert!(!settings(&mut window, |s| s.casting.looks_with("Fake")));

    click(&mut window, "pane-Hotkeys");
    click(&mut window, "restore-hotkeys");
    assert_eq!(
        recorded(&mut window),
        [Invocation::new("settings.reset").with_args(json!({"key": "hotkeys"}))]
    );
}

#[gpui::test]
fn the_command_line_pane_turns_control_on_and_installs_the_tool(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    keep_in(&mut cx, "commands-command-line");
    cx.update(|_, cx| api::record(cx));
    let mut window = open_settings(&mut cx);
    click(&mut window, "pane-Command Line");
    assert!(!settings(&mut window, |s| s.command_line_control), "off");
    click(&mut window, "command-line-control");
    assert_eq!(
        recorded(&mut window),
        [set("command_line_control", json!(true))]
    );
    assert!(settings(&mut window, |s| s.command_line_control));

    if cfg!(target_os = "macos") {
        // Noted, not run: nothing is linked into /usr/local/bin.
        cx.update(|_, cx| api::intercept(cx));
        click(&mut window, "install-cli");
        assert_eq!(recorded(&mut window), [Invocation::new("cli.install")]);
    }
}

#[gpui::test]
fn a_slider_shows_its_setting_as_dragged_and_sets_it_once_let_go(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let folder = keep_in(&mut cx, "commands-slider");
    cx.update(|_, cx| api::record(cx));
    let mut window = open_settings(&mut cx);
    click(&mut window, "pane-Audio");
    let slider = find(&mut window, "master-volume");
    let (from, to) = (
        slider.center(),
        point(slider.left() + px(1.), slider.center().y),
    );

    window.simulate_mouse_move(from, None, Modifiers::none());
    window.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    window.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    window.run_until_parked();
    let dragged = settings(&mut window, |s| s.master_volume);
    assert!(dragged < 0.1, "it follows the mouse: {dragged}");
    assert_eq!(recorded(&mut window), [], "nothing sent while dragged");
    assert_eq!(saved(&folder), None, "nor saved");

    window.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    window.run_until_parked();
    assert_eq!(
        recorded(&mut window),
        [set("master_volume", json!(dragged))]
    );
    let file: Value = serde_json::from_str(&saved(&folder).expect("saved once let go")).unwrap();
    assert_eq!(file["master_volume"], json!(dragged));
}

#[gpui::test]
fn a_bad_value_is_turned_away(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let folder = keep_in(&mut cx, "commands-bad-value");
    assert_eq!(
        run(
            &mut cx,
            "settings.set",
            json!({"key": "theme", "value": "purple"})
        ),
        Err(Error::bad_args(
            "settings.set",
            r#"theme is one of "system", "dark", "light""#
        ))
    );
    assert_eq!(settings(&mut cx, |s| s.theme), ThemeMode::System, "left be");
    assert_eq!(saved(&folder), None, "and not saved");
    assert_eq!(
        run(&mut cx, "settings.set", json!({"key": "theme"})),
        Err(Error::bad_args("settings.set", "needs value"))
    );
    assert_eq!(
        run(
            &mut cx,
            "settings.set",
            json!({"key": "colour", "value": 1})
        ),
        Err(Error::not_found("setting", "colour"))
    );
    assert_eq!(
        run(&mut cx, "settings.get", json!({"key": "colour"})),
        Err(Error::not_found("setting", "colour"))
    );
    assert_eq!(
        run(&mut cx, "settings.reset", json!({"key": "colour"})),
        Err(Error::not_found("setting", "colour"))
    );
}

/// What `settings.ls`, `listed`, says of `key`.
fn entry(listed: &Value, key: &str) -> Value {
    listed
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == key)
        .cloned()
        .unwrap_or_else(|| panic!("{key} listed"))
}

#[gpui::test]
fn lists_every_setting_with_what_it_takes(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    with_fake_protocol(&mut cx);
    let listed = run(&mut cx, "settings.ls", Value::Null).unwrap();
    assert_eq!(
        listed[0]["key"], "autoplay",
        "in the order Settings has them"
    );
    assert_eq!(
        entry(&listed, "theme"),
        json!({
            "key": "theme",
            "title": "Theme",
            "pane": "Appearance",
            "kind": "choice",
            "value": "system",
            "default": "system",
            "choices": [
                {"value": "system", "label": "System"},
                {"value": "dark", "label": "Dark"},
                {"value": "light", "label": "Light"},
            ],
        })
    );
    assert_eq!(
        entry(&listed, "subtitles.size"),
        json!({
            "key": "subtitles.size",
            "title": "Size",
            "pane": "Subtitles",
            "kind": "number",
            "value": 1.0,
            "default": 1.0,
            "min": 0.5,
            "max": 2.0,
        })
    );
    assert_eq!(
        entry(&listed, "casting.look_with.Fake"),
        json!({
            "key": "casting.look_with.Fake",
            "title": "Fake",
            "pane": "Casting",
            "kind": "toggle",
            "value": true,
            "default": true,
        })
    );
    assert_eq!(
        entry(&listed, "command_line_control"),
        json!({
            "key": "command_line_control",
            "title": "Allow control from the command line",
            "pane": "Command Line",
            "kind": "toggle",
            "value": false,
            "default": false,
        })
    );
    for (key, kind) in [
        ("subtitles.font", "font"),
        ("screenshots.folder", "folder"),
        ("audio_output.device", "output"),
        ("hotkeys", "keys"),
    ] {
        assert_eq!(entry(&listed, key)["kind"], kind, "{key}");
    }
}

#[gpui::test]
fn reads_sets_and_resets_a_setting_by_its_key(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    with_fake_protocol(&mut cx);
    let key = |key: &str| json!({"key": key});
    assert_eq!(
        run(&mut cx, "settings.get", key("theme")),
        Ok(json!("system"))
    );
    assert_eq!(
        run(
            &mut cx,
            "settings.set",
            json!({"key": "theme", "value": "dark"})
        ),
        Ok(json!("dark"))
    );
    assert_eq!(
        run(&mut cx, "settings.get", key("theme")),
        Ok(json!("dark"))
    );
    let listed = run(&mut cx, "settings.ls", Value::Null).unwrap();
    assert_eq!(entry(&listed, "theme")["value"], "dark");
    assert_eq!(entry(&listed, "theme")["default"], "system");
    assert_eq!(
        run(&mut cx, "settings.reset", key("theme")),
        Ok(json!("system"))
    );
    assert_eq!(settings(&mut cx, |s| s.theme), ThemeMode::System);

    let fake = "casting.look_with.Fake";
    assert_eq!(
        run(
            &mut cx,
            "settings.set",
            json!({"key": fake, "value": false})
        ),
        Ok(json!(false))
    );
    assert_eq!(run(&mut cx, "settings.get", key(fake)), Ok(json!(false)));
    assert_eq!(run(&mut cx, "settings.reset", key(fake)), Ok(json!(true)));
    assert!(settings(&mut cx, |s| s.casting.looks_with("Fake")));
}

#[gpui::test]
fn a_reload_picks_up_the_file(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let folder = keep_in(&mut cx, "commands-reload");
    assert_eq!(
        run(&mut cx, "settings.reload", Value::Null),
        Err(Error::failed(
            "there's no settings file yet: every setting is as Plays starts it out"
        ))
    );

    let file = folder.file("settings.json").unwrap();
    let text = r#"{"theme": "dark", "columns": 2, "subtitles": {"size": 1.5}}"#;
    crate::files::write_whole(&file, text.as_bytes()).unwrap();
    assert_eq!(
        run(&mut cx, "settings.reload", Value::Null),
        Ok(json!(["subtitles.size", "theme", "columns"])),
        "the settings it changed"
    );
    assert_eq!(
        settings(&mut cx, |s| (s.theme, s.columns, s.subtitles.size)),
        (ThemeMode::Dark, 2, 1.5)
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        text,
        "read, not saved over"
    );
    assert_eq!(
        run(&mut cx, "settings.reload", Value::Null),
        Ok(json!([])),
        "again, nothing changes"
    );

    cx.update(|_, cx| cx.set_global(Folder::app()));
    let nowhere = Err(Error::failed("the settings aren't kept in a file here"));
    assert_eq!(run(&mut cx, "settings.reload", Value::Null), nowhere);
    assert_eq!(run(&mut cx, "settings.reveal_file", Value::Null), nowhere);
}

#[gpui::test]
fn reveals_the_settings_file_saving_it_first(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let folder = keep_in(&mut cx, "commands-reveal");
    let file = folder.file("settings.json").unwrap();
    assert_eq!(
        run(&mut cx, "settings.reveal_file", Value::Null),
        Ok(json!(file.display().to_string()))
    );
    let saved = saved(&folder).expect("saved first");
    assert_eq!(saved, settings(&mut cx, Settings::to_json));
}

#[gpui::test]
fn copies_the_version_and_what_a_bug_report_needs(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let clipboard = |cx: &mut VisualTestContext| {
        TestAppContext::read(cx, |cx| cx.read_from_clipboard()?.text())
    };
    let version = format!(
        "Plays {} ({})",
        env!("CARGO_PKG_VERSION"),
        about::release_date()
    );
    assert_eq!(
        run(&mut cx, "app.copy_version_info", Value::Null),
        Ok(json!(version))
    );
    assert_eq!(clipboard(&mut cx), Some(version.clone()));

    let clip = testing::fixture("clip.mp4");
    run(&mut cx, "app.open", json!({"urls": [clip]})).unwrap();
    let sound = cx.update(|_, cx| AudioEngine::device_description(cx));
    let system = format!(
        "System: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let mut expected = vec![
        version.clone(),
        system.clone(),
        format!("FFmpeg: {}", ffmpeg_version()),
        format!("Sound: {sound}"),
        "Windows: 1, tiles: 1".to_owned(),
        "Settings: as Plays starts them out".to_owned(),
    ];
    let copied = run(&mut cx, "app.copy_debug_info", Value::Null).unwrap();
    assert_eq!(copied, json!(expected.join("\n")));
    assert_eq!(clipboard(&mut cx), Some(expected.join("\n")));

    run(
        &mut cx,
        "settings.set",
        json!({"key": "theme", "value": "dark"}),
    )
    .unwrap();
    run(
        &mut cx,
        "settings.set",
        json!({"key": "subtitles.size", "value": 1.5}),
    )
    .unwrap();
    expected.pop();
    expected.extend([
        "Settings changed:".to_owned(),
        "  subtitles.size: 1.5".to_owned(),
        "  theme: \"dark\"".to_owned(),
    ]);
    assert_eq!(
        run(&mut cx, "app.copy_debug_info", Value::Null),
        Ok(json!(expected.join("\n")))
    );
}
