use std::collections::HashSet;

use gpui::{KeyContext, Keymap, Keystroke};
use serde_json::json;

use super::*;

/// The command `key` runs, pressed in `contexts`.
fn bound(keymap: &Keymap, key: &str, contexts: &[KeyContext]) -> Option<String> {
    let keys = [Keystroke::parse(key).unwrap()];
    let (found, _) = keymap.bindings_for_input(&keys, contexts);
    found.first().map(|binding| {
        let run = binding.action().as_any().downcast_ref::<Run>();
        run.expect("keys run commands").0.command.clone()
    })
}

/// The first command `key` is for, if any.
fn command_for(hotkeys: &Hotkeys, key: &str) -> Option<&'static str> {
    hotkeys
        .0
        .iter()
        .find(|(_, keys)| keys.iter().any(|k| k == key))
        .map(|(command, _)| *command)
}

fn context(names: &str) -> KeyContext {
    KeyContext::parse(names).unwrap()
}

#[test]
fn every_command_that_takes_keys_has_its_own_and_they_dont_clash() {
    let hotkeys = Hotkeys::default();
    for spec in api::all() {
        assert_eq!(
            hotkeys.0.contains_key(spec.id),
            spec.takes_keys(),
            "{}",
            spec.id
        );
    }
    // The app's, a window's, and those keys never ran before.
    assert_eq!(
        hotkeys.keys("app.settings.open"),
        [normalize("secondary-,").unwrap()]
    );
    assert_eq!(
        hotkeys.keys("window.open_files"),
        [normalize("secondary-o").unwrap()]
    );
    assert_eq!(hotkeys.keys("app.playback.play"), [] as [String; 0]);
    assert!(hotkeys.0.contains_key("app.playback.play"));
    // Needing what they act on named, or arguments, they take none.
    assert!(!hotkeys.0.contains_key("window.focus"));
    assert!(!hotkeys.0.contains_key("window.show_on_display"));

    let mut seen = HashSet::new();
    for spec in commands() {
        assert_eq!(
            hotkeys.keys(spec.id).len(),
            spec.keys.len(),
            "{} has a key that isn't one",
            spec.id
        );
        for key in hotkeys.keys(spec.id) {
            assert!(
                seen.insert((key.clone(), in_players(spec.reach))),
                "{key} is used twice"
            );
        }
    }
    assert_eq!(hotkeys.keys("block.playback.play_pause"), ["space", "p"]);
    assert_eq!(
        command_for(&hotkeys, "shift-g"),
        Some("block.subtitles.bigger")
    );
}

#[test]
fn binds_each_key_where_its_command_works() {
    let keymap = Keymap::new(Hotkeys::default().bindings());
    let root = KeyContext::new_with_defaults();
    let player = [root.clone(), context("Workspace")];
    let typing = [root.clone(), context("Workspace"), context("TextInput")];
    let elsewhere = [root.clone(), context("Settings")];
    let recording = [root.clone(), context("Settings Recording")];
    // The palette over a player window, waiting for keys.
    let palette_recording = [
        root,
        context("Workspace"),
        context("CommandPalette Recording"),
    ];
    // An empty player window sets no context.
    let empty: [KeyContext; 0] = [];
    let run = |key: &str, contexts: &[KeyContext]| bound(&keymap, key, contexts);

    // A player window's keys: there, but not while typing, nor elsewhere.
    assert_eq!(
        run("space", &player).as_deref(),
        Some("block.playback.play_pause")
    );
    assert_eq!(run("space", &typing), None);
    assert_eq!(run("space", &elsewhere), None);
    assert_eq!(
        run("secondary-shift-l", &player).as_deref(),
        Some("block.playback.loop.toggle")
    );
    assert_eq!(run("secondary-shift-l", &typing), None);
    assert_eq!(run("space", &empty), None);
    // ⌘W: the file in front in a player window with files; else, typing
    // in one too, the window in front.
    assert_eq!(run("secondary-w", &player).as_deref(), Some("block.close"));
    assert_eq!(run("secondary-w", &typing).as_deref(), Some("window.close"));
    assert_eq!(
        run("secondary-w", &elsewhere).as_deref(),
        Some("window.close")
    );
    assert_eq!(run("secondary-w", &empty).as_deref(), Some("window.close"));
    assert_eq!(
        run("secondary-shift-w", &typing).as_deref(),
        Some("block.close_all")
    );
    assert_eq!(run("secondary-shift-w", &elsewhere), None);
    // ⌘O anywhere, typing too, but not while Settings records a key.
    for contexts in [&player[..], &typing, &elsewhere, &empty] {
        assert_eq!(
            run("secondary-o", contexts).as_deref(),
            Some("window.open_files")
        );
        assert_eq!(run("ctrl-tab", contexts).as_deref(), Some("window.next"));
    }
    assert_eq!(run("secondary-o", &recording), None);
    // None of them while the palette records a key over a player window.
    for key in ["space", "secondary-shift-l", "secondary-w", "secondary-o"] {
        assert_eq!(run(key, &palette_recording), None, "{key}");
    }

    // With the window's Close given other keys, ⌘W closes the file in
    // front while typing too.
    let mut hotkeys = Hotkeys::default();
    hotkeys.remove("window.close", &normalize("secondary-w").unwrap());
    let keymap = Keymap::new(hotkeys.bindings());
    assert_eq!(
        bound(&keymap, "secondary-w", &typing).as_deref(),
        Some("block.close")
    );

    // A key that types, given a command that works anywhere or on the
    // file in front, leaves typing alone.
    let mut hotkeys = Hotkeys::default();
    hotkeys.add("app.settings.open", "k");
    hotkeys.add("block.close", "x");
    let keymap = Keymap::new(hotkeys.bindings());
    assert_eq!(bound(&keymap, "x", &player).as_deref(), Some("block.close"));
    assert_eq!(bound(&keymap, "x", &typing), None);
    assert_eq!(
        bound(&keymap, "k", &elsewhere).as_deref(),
        Some("app.settings.open")
    );
    assert_eq!(
        bound(&keymap, "k", &empty).as_deref(),
        Some("app.settings.open")
    );
    assert_eq!(bound(&keymap, "k", &typing), None);
    assert_eq!(bound(&keymap, "k", &recording), None);
}

#[test]
fn moves_a_key_to_its_new_command() {
    let mut hotkeys = Hotkeys::default();
    assert_eq!(
        hotkeys.add("block.playback.restart", "p"),
        Some("block.playback.play_pause")
    );
    assert_eq!(hotkeys.keys("block.playback.play_pause"), ["space"]);
    assert_eq!(hotkeys.keys("block.playback.restart"), ["home", "p"]);
    assert_eq!(hotkeys.add("block.playback.restart", "home"), None);
    assert_eq!(hotkeys.keys("block.playback.restart"), ["p", "home"]);
    hotkeys.remove("block.playback.restart", "p");
    assert_eq!(hotkeys.keys("block.playback.restart"), ["home"]);
    assert_eq!(hotkeys.add("block.playback.restart", "not a key"), None);
    assert_eq!(hotkeys.keys("block.playback.restart"), ["home"]);

    // One that takes no keys gets none.
    assert_eq!(hotkeys.add("window.focus", "g"), None);
    assert_eq!(command_for(&hotkeys, "g"), None);
}

#[test]
fn a_key_moves_only_among_commands_that_work_where_it_does() {
    let mut hotkeys = Hotkeys::default();
    let w = normalize("secondary-w").unwrap();
    // Given back to the file in front, the window in front keeps it.
    hotkeys.remove("block.close", &w);
    assert_eq!(hotkeys.add("block.close", &w), None);
    assert_eq!(hotkeys.keys("window.close").last(), Some(&w));
    // Given to another in a player window, the file in front's goes.
    assert_eq!(
        hotkeys.add("block.playback.restart", &w),
        Some("block.close")
    );
    assert!(!hotkeys.keys("block.close").contains(&w));
    assert!(hotkeys.keys("window.close").contains(&w));
    // And to another that works anywhere, the window in front's.
    assert_eq!(hotkeys.add("app.about", &w), Some("window.close"));
    assert!(!hotkeys.keys("window.close").contains(&w));
    assert_eq!(hotkeys.keys("app.about"), std::slice::from_ref(&w));
    assert_eq!(hotkeys.keys("block.playback.restart").last(), Some(&w));
}

#[test]
fn reads_and_writes_the_settings_file() {
    let mut hotkeys = Hotkeys::default();
    hotkeys.add("block.audio.mute.toggle", "shift-x");
    hotkeys.remove("block.playback.play_pause", "p");
    hotkeys.add("window.open_files", "secondary-shift-k");
    let saved = serde_json::to_value(&hotkeys).unwrap();
    // Under the names they had before commands had ids, so files saved
    // then keep them; others under their ids.
    let names: Vec<&str> = saved
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(names, ["mute", "play_pause", "window.open_files"]);
    assert_eq!(saved["play_pause"], json!(["space"]));
    assert_eq!(serde_json::from_value::<Hotkeys>(saved).unwrap(), hotkeys);

    // Missing commands keep their defaults; unknown ones and bad keys go;
    // a command may be named by its id.
    let read: Hotkeys = serde_json::from_str(
        r#"{"play_pause": ["k", "bad key"], "dance": ["d"], "loop": ["cmd-l"],
            "block.playback.restart": ["shift-k"]}"#,
    )
    .unwrap();
    assert_eq!(read.keys("block.playback.play_pause"), ["k"]);
    assert_eq!(read.keys("block.playback.loop.toggle"), ["cmd-l"]);
    assert_eq!(read.keys("block.playback.restart"), ["shift-k"]);
    assert_eq!(
        read.keys("block.audio.mute.toggle"),
        Hotkeys::default().keys("block.audio.mute.toggle")
    );
    assert_eq!(read.0.len(), commands().count());
}

#[test]
fn every_name_saved_before_commands_had_ids_still_names_one() {
    let earlier = [
        "play_pause",
        "seek_forward",
        "seek_backward",
        "next_frame",
        "previous_frame",
        "next_chapter",
        "previous_chapter",
        "restart",
        "previous_file",
        "next_file",
        "loop",
        "volume_up",
        "volume_down",
        "mute",
        "next_audio_track",
        "audio_earlier",
        "audio_later",
        "reset_audio_delay",
        "next_subtitle_track",
        "toggle_subtitles",
        "bigger_subtitles",
        "smaller_subtitles",
        "subtitles_up",
        "subtitles_down",
        "subtitles_earlier",
        "subtitles_later",
        "reset_subtitle_delay",
        "next_video_track",
        "rotate_right",
        "rotate_left",
        "flip_horizontally",
        "flip_vertically",
        "next_aspect_ratio",
        "next_crop",
        "reset_picture",
        "take_screenshot",
        "full_screen",
        "exit_full_screen",
        "match_video_size",
        "picture_in_picture",
        "float_on_top",
        "float_on_top_while_playing",
        "console",
        "focus_left",
        "focus_right",
        "focus_up",
        "focus_down",
        "focus_previous_block",
        "focus_next_block",
        "move_block_left",
        "move_block_right",
        "move_block_up",
        "move_block_down",
        "play_pause_all",
        "seek_forward_all",
        "seek_backward_all",
        "next_frame_all",
        "previous_frame_all",
        "restart_all",
        "sync_all",
        "volume_up_all",
        "volume_down_all",
        "mute_all",
    ];
    for name in earlier {
        let read: Hotkeys = serde_json::from_value(json!({ name: ["f13"] })).unwrap();
        let named = read
            .0
            .values()
            .filter(|keys| keys.contains(&"f13".to_owned()));
        assert_eq!(named.count(), 1, "{name}");
    }
}

#[test]
fn saves_only_changed_keys_so_new_defaults_arrive() {
    assert_eq!(serde_json::to_string(&Hotkeys::default()).unwrap(), "{}");
    let mut hotkeys = Hotkeys::default();
    hotkeys.remove("block.playback.restart", "home");
    assert_eq!(
        serde_json::to_string(&hotkeys).unwrap(),
        r#"{"restart":[]}"#
    );
    // A file saved with every command, before `<` and `>` stepped frames.
    let read: Hotkeys =
        serde_json::from_str(r#"{"next_frame": ["alt-right", "."], "previous_frame": [","]}"#)
            .unwrap();
    assert_eq!(
        read.keys("block.playback.next_frame"),
        ["alt-right", ".", ">"]
    );
    assert_eq!(read.keys("block.playback.previous_frame"), [","]);
}

#[test]
fn shows_keys() {
    if cfg!(target_os = "macos") {
        assert_eq!(display("ctrl-cmd-f"), "⌃⌘F");
        assert_eq!(display("cmd-shift-l"), "⇧⌘L");
        assert_eq!(display("alt-right"), "⌥→");
        assert_eq!(display("escape"), "⎋");
    } else {
        assert_eq!(display("ctrl-alt-f"), "Ctrl+Alt+F");
        assert_eq!(display("shift-home"), "Shift+Home");
    }
    assert_eq!(display("space"), "Space");
    assert_eq!(display("f11"), "F11");
    assert_eq!(display("."), ".");
}
