//! The shortcuts that control playback, which can be changed in Settings.
//! Each command starts out with its default keys, among them a few for
//! every file in a window at once.
//!
//! Each command has a gpui action, bound in the `Workspace` key context so
//! single keys such as space don't fire while typing elsewhere. Keys are
//! stored as gpui keystrokes, such as `cmd-shift-l`.

use std::collections::BTreeMap;

use gpui::{Action, DummyKeyboardMapper, KeyBinding, KeyBindingContextPredicate, Keystroke};
use serde::{Deserialize, Serialize};

/// Every command once, in the order Settings shows them, under its group:
/// its variant, the gpui action it fires, its name in the settings file and
/// its title. The actions, `Command`, `Command::ALL`, `id`, `title`, `group`
/// and `action` are all made from this one list, so a command can't be
/// missing from any of them. Its default keys are a `match` of their own
/// (`Command::default_keys`), which must name every command too.
macro_rules! commands {
    ($($group:ident { $($command:ident => $action:ident, $id:literal, $title:literal;)+ })+) => {
        gpui::actions!(plays, [$($($action,)+)+]);

        /// What a shortcut does.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Command {
            $($($command,)+)+
        }

        impl Command {
            /// Every command, in the order Settings shows them.
            pub const ALL: [Self; [$($(stringify!($command),)+)+].len()] =
                [$($(Self::$command,)+)+];

            /// Its name in the settings file.
            pub fn id(self) -> &'static str {
                match self {
                    $($(Self::$command => $id,)+)+
                }
            }

            pub fn title(self) -> &'static str {
                match self {
                    $($(Self::$command => $title,)+)+
                }
            }

            pub fn group(self) -> Group {
                match self {
                    $($(Self::$command)|+ => Group::$group,)+
                }
            }

            pub fn action(self) -> Box<dyn Action> {
                match self {
                    $($(Self::$command => Box::new($action),)+)+
                }
            }
        }
    };
}

commands! {
    Playback {
        PlayPause => PlayPause, "play_pause", "Play/pause";
        SeekForward => SeekForward, "seek_forward", "Seek forward";
        SeekBackward => SeekBackward, "seek_backward", "Seek backward";
        NextFrame => NextFrame, "next_frame", "Next frame";
        PreviousFrame => PreviousFrame, "previous_frame", "Previous frame";
        NextChapter => NextChapter, "next_chapter", "Next chapter";
        PreviousChapter => PreviousChapter, "previous_chapter", "Previous chapter";
        Restart => Restart, "restart", "Restart";
        PreviousFile => PreviousFile, "previous_file", "Previous file in its folder";
        NextFile => NextFile, "next_file", "Next file in its folder";
        Loop => ToggleLoop, "loop", "Loop";
    }
    Audio {
        VolumeUp => VolumeUp, "volume_up", "Volume up";
        VolumeDown => VolumeDown, "volume_down", "Volume down";
        Mute => ToggleMute, "mute", "Mute";
        NextAudioTrack => NextAudioTrack, "next_audio_track", "Next audio track";
        AudioEarlier => AudioEarlier, "audio_earlier", "Audio earlier";
        AudioLater => AudioLater, "audio_later", "Audio later";
        ResetAudioDelay => ResetAudioDelay, "reset_audio_delay", "Reset audio delay";
    }
    Subtitles {
        NextSubtitleTrack => NextSubtitleTrack, "next_subtitle_track", "Next subtitles";
        ToggleSubtitles => ToggleSubtitles, "toggle_subtitles", "Show/hide subtitles";
        BiggerSubtitles => BiggerSubtitles, "bigger_subtitles", "Bigger subtitles";
        SmallerSubtitles => SmallerSubtitles, "smaller_subtitles", "Smaller subtitles";
        SubtitlesUp => SubtitlesUp, "subtitles_up", "Move subtitles up";
        SubtitlesDown => SubtitlesDown, "subtitles_down", "Move subtitles down";
        SubtitlesEarlier => SubtitlesEarlier, "subtitles_earlier", "Subtitles earlier";
        SubtitlesLater => SubtitlesLater, "subtitles_later", "Subtitles later";
        ResetSubtitleDelay => ResetSubtitleDelay, "reset_subtitle_delay", "Reset subtitle delay";
    }
    Video {
        NextVideoTrack => NextVideoTrack, "next_video_track", "Next video track";
        RotateRight => RotateRight, "rotate_right", "Rotate right";
        RotateLeft => RotateLeft, "rotate_left", "Rotate left";
        FlipHorizontally => FlipHorizontally, "flip_horizontally", "Flip horizontally";
        FlipVertically => FlipVertically, "flip_vertically", "Flip vertically";
        NextAspectRatio => NextAspectRatio, "next_aspect_ratio", "Next aspect ratio";
        NextCrop => NextCrop, "next_crop", "Next crop";
        ResetPicture => ResetPicture, "reset_picture", "Reset picture";
        TakeScreenshot => TakeScreenshot, "take_screenshot", "Take screenshot";
    }
    Window {
        FullScreen => ToggleFullScreen, "full_screen", "Full screen";
        ExitFullScreen => ExitFullScreen, "exit_full_screen", "Exit full screen";
        PictureInPicture => TogglePictureInPicture, "picture_in_picture", "Picture in Picture";
        FloatOnTop => ToggleFloatOnTop, "float_on_top", "Float on top";
        FloatOnTopWhilePlaying => ToggleFloatOnTopWhilePlaying,
            "float_on_top_while_playing", "Float on top while playing";
        Console => ToggleConsole, "console", "Show/hide console";
    }
    AllFiles {
        PlayPauseAll => PlayPauseAll, "play_pause_all", "Play/pause all";
        SeekForwardAll => SeekForwardAll, "seek_forward_all", "Seek all forward";
        SeekBackwardAll => SeekBackwardAll, "seek_backward_all", "Seek all backward";
        NextFrameAll => NextFrameAll, "next_frame_all", "Next frame on all";
        PreviousFrameAll => PreviousFrameAll, "previous_frame_all", "Previous frame on all";
        RestartAll => RestartAll, "restart_all", "Restart all";
        SyncAll => SyncAll, "sync_all", "Sync all to the tile in front";
        VolumeUpAll => VolumeUpAll, "volume_up_all", "Volume up on all";
        VolumeDownAll => VolumeDownAll, "volume_down_all", "Volume down on all";
        MuteAll => ToggleMuteAll, "mute_all", "Mute/unmute all";
    }
}

// Every tile in the window, from the Playback menu only: no shortcut is
// offered for them.
gpui::actions!(plays, [PlayAll, PauseAll]);

/// The key context the shortcuts work in: a window with files open, but
/// not while typing in a text field there (the stream URL), so Space and
/// letters type.
pub const CONTEXT: &str = "Workspace && !TextInput";

/// How the commands are grouped in Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Playback,
    Audio,
    Subtitles,
    Video,
    Window,
    AllFiles,
}

impl Group {
    pub const ALL: [Self; 6] = [
        Self::Playback,
        Self::Audio,
        Self::Subtitles,
        Self::Video,
        Self::Window,
        Self::AllFiles,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Playback => "Playback",
            Self::Audio => "Audio",
            Self::Subtitles => "Subtitles",
            Self::Video => "Video",
            Self::Window => "Window",
            Self::AllFiles => "All files in a window",
        }
    }
}

impl Command {
    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    /// Whether this system has what it does: Picture in Picture is
    /// macOS's.
    pub fn available(self) -> bool {
        self != Self::PictureInPicture || cfg!(target_os = "macos")
    }

    /// The keys a command starts out with. `secondary` is ⌘ on macOS and
    /// Ctrl elsewhere.
    fn default_keys(self) -> &'static [&'static str] {
        let mac = cfg!(target_os = "macos");
        match self {
            Self::PlayPause => &["space", "p"],
            Self::SeekForward => &["right"],
            Self::SeekBackward => &["left"],
            Self::NextFrame => &["alt-right", ".", ">"],
            Self::PreviousFrame => &["alt-left", ",", "<"],
            Self::NextChapter => &["pagedown"],
            Self::PreviousChapter => &["pageup"],
            Self::Restart => &["home"],
            Self::PreviousFile => &["secondary-left"],
            Self::NextFile => &["secondary-right"],
            Self::Loop => &["secondary-shift-l"],
            Self::VolumeUp => &["up"],
            Self::VolumeDown => &["down"],
            Self::Mute => &["secondary-/", "m"],
            Self::NextAudioTrack if mac => &["ctrl-cmd-a"],
            Self::NextAudioTrack => &["ctrl-alt-a"],
            Self::AudioEarlier => &["ctrl--"],
            Self::AudioLater => &["ctrl-="],
            Self::ResetAudioDelay => &["ctrl-backspace"],
            Self::NextSubtitleTrack if mac => &["ctrl-cmd-s"],
            Self::NextSubtitleTrack => &["ctrl-alt-s"],
            Self::NextVideoTrack if mac => &["ctrl-cmd-v"],
            Self::NextVideoTrack => &["ctrl-alt-v"],
            Self::ToggleSubtitles => &["v"],
            Self::BiggerSubtitles => &["shift-g"],
            Self::SmallerSubtitles => &["shift-f"],
            Self::SubtitlesUp => &["r"],
            Self::SubtitlesDown => &["shift-r", "t"],
            Self::SubtitlesEarlier => &["z"],
            Self::SubtitlesLater => &["x"],
            Self::ResetSubtitleDelay => &["shift-backspace"],
            Self::RotateRight => &["secondary-r"],
            Self::RotateLeft => &["secondary-l"],
            Self::FlipHorizontally => &["secondary-shift-h"],
            Self::FlipVertically => &["secondary-shift-v"],
            Self::NextAspectRatio => &["a"],
            Self::NextCrop => &["c"],
            Self::ResetPicture => &["secondary-0"],
            Self::TakeScreenshot => &["s"],
            Self::FullScreen if mac => &["ctrl-cmd-f", "f"],
            Self::FullScreen => &["f11", "f"],
            Self::ExitFullScreen => &["escape"],
            Self::PictureInPicture if mac => &["ctrl-cmd-p"],
            // Picture in Picture is macOS's.
            Self::PictureInPicture => &[],
            // Set in Settings or the Video menu, or given keys there.
            Self::FloatOnTop | Self::FloatOnTopWhilePlaying => &[],
            Self::Console => &["~", "`"],
            // Every file in the window.
            Self::PlayPauseAll => &["shift-space"],
            Self::RestartAll => &["shift-home"],
            Self::SeekForwardAll => &["shift-right"],
            Self::SeekBackwardAll => &["shift-left"],
            Self::NextFrameAll => &["alt-shift-right"],
            Self::PreviousFrameAll => &["alt-shift-left"],
            Self::SyncAll => &["shift-s"],
            Self::VolumeUpAll => &["shift-up"],
            Self::VolumeDownAll => &["shift-down"],
            Self::MuteAll => &["shift-m"],
        }
    }
}

/// Keys commands started out with before they had the ones they have now.
/// A settings file that still has them gets today's instead.
const EARLIER_DEFAULTS: &[(Command, &[&str])] = &[
    (Command::NextFrame, &["alt-right", "."]),
    (Command::PreviousFrame, &["alt-left", ","]),
];

/// The keys for every command.
#[derive(Clone, Debug, PartialEq)]
pub struct Hotkeys(BTreeMap<Command, Vec<String>>);

impl Default for Hotkeys {
    fn default() -> Self {
        Self(
            Command::ALL
                .into_iter()
                .map(|command| (command, default_keys(command)))
                .collect(),
        )
    }
}

fn default_keys(command: Command) -> Vec<String> {
    command
        .default_keys()
        .iter()
        .filter_map(|key| normalize(key))
        .collect()
}

/// A keystroke as gpui writes it, so the same keys compare equal; `None`
/// if it isn't one.
pub fn normalize(key: &str) -> Option<String> {
    // A space would make it a sequence of keystrokes, which isn't offered.
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Keystroke::parse(key)
        .ok()
        .filter(|keystroke| !keystroke.key.is_empty())
        .map(|keystroke| keystroke.unparse())
}

impl Hotkeys {
    pub fn keys(&self, command: Command) -> &[String] {
        self.0.get(&command).map_or(&[], Vec::as_slice)
    }

    /// The command `key` is for, if any.
    pub fn command_for(&self, key: &str) -> Option<Command> {
        self.0
            .iter()
            .find(|(_, keys)| keys.iter().any(|k| k == key))
            .map(|(command, _)| *command)
    }

    /// Give `command` the key, taking it from the command that had it,
    /// which is returned.
    pub fn add(&mut self, command: Command, key: &str) -> Option<Command> {
        let key = normalize(key)?;
        let previous = self.command_for(&key).filter(|c| *c != command);
        for keys in self.0.values_mut() {
            keys.retain(|k| *k != key);
        }
        self.0.entry(command).or_default().push(key);
        previous
    }

    pub fn remove(&mut self, command: Command, key: &str) {
        if let Some(keys) = self.0.get_mut(&command) {
            keys.retain(|k| k != key);
        }
    }

    /// Key bindings for gpui's keymap.
    pub fn bindings(&self) -> Vec<KeyBinding> {
        let context = KeyBindingContextPredicate::parse(CONTEXT)
            .ok()
            .map(Into::into);
        self.0
            .iter()
            .flat_map(|(command, keys)| keys.iter().map(move |key| (*command, key)))
            .filter_map(|(command, key)| {
                KeyBinding::load(
                    key,
                    command.action(),
                    context.clone(),
                    false,
                    None,
                    &DummyKeyboardMapper,
                )
                .inspect_err(|err| log::warn!("skipping the shortcut {key}: {err}"))
                .ok()
            })
            .collect()
    }
}

/// Only the commands whose keys were changed, so the others get new
/// defaults as they come.
impl Serialize for Hotkeys {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let map: BTreeMap<&str, &Vec<String>> = self
            .0
            .iter()
            .filter(|(command, keys)| **keys != default_keys(**command))
            .map(|(command, keys)| (command.id(), keys))
            .collect();
        map.serialize(serializer)
    }
}

/// Commands the file doesn't mention keep their defaults, so new ones get
/// theirs, as do ones it gives keys they no longer start out with; ones it
/// names that no longer exist, and keys that aren't, are left out.
impl<'de> Deserialize<'de> for Hotkeys {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = BTreeMap::<String, Vec<String>>::deserialize(deserializer)?;
        let mut hotkeys = Self::default();
        for (id, keys) in map {
            if let Some(command) = Command::from_id(&id) {
                let keys: Vec<String> = keys.iter().filter_map(|k| normalize(k)).collect();
                if !is_earlier_default(command, &keys) {
                    hotkeys.0.insert(command, keys);
                }
            }
        }
        Ok(hotkeys)
    }
}

fn is_earlier_default(command: Command, keys: &[String]) -> bool {
    EARLIER_DEFAULTS
        .iter()
        .filter(|(earlier, _)| *earlier == command)
        .any(|(_, defaults)| {
            defaults.len() == keys.len()
                && defaults
                    .iter()
                    .zip(keys)
                    .all(|(default, key)| normalize(default).as_ref() == Some(key))
        })
}

/// How a keystroke is shown: `⌃⌘F` on macOS, `Ctrl+Alt+F` elsewhere.
pub fn display(key: &str) -> String {
    let Ok(keystroke) = Keystroke::parse(key) else {
        return key.to_owned();
    };
    let modifiers = keystroke.modifiers;
    let name = key_name(&keystroke.key);
    if cfg!(target_os = "macos") {
        let mut text = String::new();
        for (on, symbol) in [
            (modifiers.control, "⌃"),
            (modifiers.alt, "⌥"),
            (modifiers.shift, "⇧"),
            (modifiers.platform, "⌘"),
            (modifiers.function, "fn "),
        ] {
            if on {
                text.push_str(symbol);
            }
        }
        text + &name
    } else {
        let mut parts: Vec<&str> = [
            (modifiers.control, "Ctrl"),
            (modifiers.alt, "Alt"),
            (modifiers.shift, "Shift"),
            (modifiers.platform, "Super"),
            (modifiers.function, "Fn"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect();
        parts.push(&name);
        parts.join("+")
    }
}

fn key_name(key: &str) -> String {
    let mac = cfg!(target_os = "macos");
    let name = match key {
        "space" => "Space",
        "left" => "←",
        "right" => "→",
        "up" => "↑",
        "down" => "↓",
        "enter" if mac => "↩",
        "enter" => "Enter",
        "escape" if mac => "⎋",
        "escape" => "Esc",
        "backspace" if mac => "⌫",
        "backspace" => "Backspace",
        "delete" if mac => "⌦",
        "delete" => "Delete",
        "tab" if mac => "⇥",
        "tab" => "Tab",
        "home" if mac => "↖",
        "home" => "Home",
        "end" if mac => "↘",
        "end" => "End",
        "pageup" if mac => "⇞",
        "pageup" => "PgUp",
        "pagedown" if mac => "⇟",
        "pagedown" => "PgDn",
        other if other.chars().count() == 1 => return other.to_uppercase(),
        // F1 and the like.
        other if other.starts_with('f') && other[1..].parse::<u8>().is_ok() => {
            return other.to_uppercase();
        }
        other => other,
    };
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn defaults_are_keys_and_dont_clash() {
        let hotkeys = Hotkeys::default();
        let mut seen = HashSet::new();
        for command in Command::ALL {
            assert_eq!(
                hotkeys.keys(command).len(),
                command.default_keys().len(),
                "{command:?} has a key that isn't one"
            );
            for key in hotkeys.keys(command) {
                assert!(seen.insert(key.clone()), "{key} is used twice");
            }
        }
        assert_eq!(hotkeys.keys(Command::PlayPause), ["space", "p"]);
        assert_eq!(
            hotkeys.command_for("shift-g"),
            Some(Command::BiggerSubtitles)
        );
        // Every command has a group and an id of its own.
        let ids: HashSet<_> = Command::ALL.iter().map(|c| c.id()).collect();
        assert_eq!(ids.len(), Command::ALL.len());
        for group in Group::ALL {
            assert!(Command::ALL.iter().any(|c| c.group() == group));
        }
    }

    #[test]
    fn leaves_text_fields_alone() {
        let predicate = KeyBindingContextPredicate::parse(CONTEXT).unwrap();
        let workspace = gpui::KeyContext::parse("Workspace").unwrap();
        let tile = gpui::KeyContext::parse("PlayerView").unwrap();
        let field = gpui::KeyContext::parse("TextInput").unwrap();
        assert!(predicate.depth_of(&[workspace.clone(), tile]).is_some());
        assert!(predicate.depth_of(&[workspace, field]).is_none());
        assert_eq!(
            Hotkeys::default().bindings().len(),
            Command::ALL
                .iter()
                .map(|c| c.default_keys().len())
                .sum::<usize>()
        );
    }

    #[test]
    fn moves_a_key_to_its_new_command() {
        let mut hotkeys = Hotkeys::default();
        assert_eq!(hotkeys.add(Command::Restart, "p"), Some(Command::PlayPause));
        assert_eq!(hotkeys.keys(Command::PlayPause), ["space"]);
        assert_eq!(hotkeys.keys(Command::Restart), ["home", "p"]);
        assert_eq!(hotkeys.add(Command::Restart, "home"), None);
        assert_eq!(hotkeys.keys(Command::Restart), ["p", "home"]);
        hotkeys.remove(Command::Restart, "p");
        assert_eq!(hotkeys.keys(Command::Restart), ["home"]);
        assert_eq!(hotkeys.add(Command::Restart, "not a key"), None);
        assert_eq!(hotkeys.keys(Command::Restart), ["home"]);
    }

    #[test]
    fn reads_and_writes_the_settings_file() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.add(Command::Mute, "shift-x");
        hotkeys.remove(Command::PlayPause, "p");
        let json = serde_json::to_string(&hotkeys).unwrap();
        assert!(json.contains(r#""mute":["#));
        assert_eq!(serde_json::from_str::<Hotkeys>(&json).unwrap(), hotkeys);

        // Missing commands keep their defaults; unknown ones and bad keys go.
        let read: Hotkeys =
            serde_json::from_str(r#"{"play_pause": ["k", "bad key"], "dance": ["d"]}"#).unwrap();
        assert_eq!(read.keys(Command::PlayPause), ["k"]);
        assert_eq!(
            read.keys(Command::Mute),
            Hotkeys::default().keys(Command::Mute)
        );
        assert!(read.0.len() == Command::ALL.len());
    }

    #[test]
    fn saves_only_changed_keys_so_new_defaults_arrive() {
        assert_eq!(serde_json::to_string(&Hotkeys::default()).unwrap(), "{}");
        let mut hotkeys = Hotkeys::default();
        hotkeys.remove(Command::Restart, "home");
        assert_eq!(
            serde_json::to_string(&hotkeys).unwrap(),
            r#"{"restart":[]}"#
        );
        // A file saved with every command, before `<` and `>` stepped frames.
        let read: Hotkeys =
            serde_json::from_str(r#"{"next_frame": ["alt-right", "."], "previous_frame": [","]}"#)
                .unwrap();
        assert_eq!(read.keys(Command::NextFrame), ["alt-right", ".", ">"]);
        assert_eq!(read.keys(Command::PreviousFrame), [","]);
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
}
