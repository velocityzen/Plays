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

gpui::actions!(
    plays,
    [
        PlayPause,
        SeekForward,
        SeekBackward,
        NextFrame,
        PreviousFrame,
        Restart,
        PreviousFile,
        NextFile,
        ToggleLoop,
        VolumeUp,
        VolumeDown,
        ToggleMute,
        NextAudioTrack,
        AudioEarlier,
        AudioLater,
        ResetAudioDelay,
        NextSubtitleTrack,
        NextVideoTrack,
        ToggleSubtitles,
        BiggerSubtitles,
        SmallerSubtitles,
        SubtitlesUp,
        SubtitlesDown,
        SubtitlesEarlier,
        SubtitlesLater,
        ResetSubtitleDelay,
        RotateRight,
        RotateLeft,
        FlipHorizontally,
        FlipVertically,
        NextAspectRatio,
        NextCrop,
        ResetPicture,
        ToggleFullScreen,
        ExitFullScreen,
        PlayPauseAll,
        PlayAll,
        PauseAll,
        RestartAll,
        SeekForwardAll,
        SeekBackwardAll,
        ToggleMuteAll,
    ]
);

/// The key context the shortcuts work in: a window with files open, but
/// not while typing in a text field there (the stream URL), so Space and
/// letters type.
pub const CONTEXT: &str = "Workspace && !TextInput";

/// What a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Command {
    PlayPause,
    SeekForward,
    SeekBackward,
    NextFrame,
    PreviousFrame,
    Restart,
    PreviousFile,
    NextFile,
    Loop,
    VolumeUp,
    VolumeDown,
    Mute,
    NextAudioTrack,
    AudioEarlier,
    AudioLater,
    ResetAudioDelay,
    NextSubtitleTrack,
    NextVideoTrack,
    ToggleSubtitles,
    BiggerSubtitles,
    SmallerSubtitles,
    SubtitlesUp,
    SubtitlesDown,
    SubtitlesEarlier,
    SubtitlesLater,
    ResetSubtitleDelay,
    RotateRight,
    RotateLeft,
    FlipHorizontally,
    FlipVertically,
    NextAspectRatio,
    NextCrop,
    ResetPicture,
    FullScreen,
    ExitFullScreen,
    PlayPauseAll,
    RestartAll,
    SeekForwardAll,
    SeekBackwardAll,
    MuteAll,
}

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
    pub const ALL: [Self; 40] = [
        Self::PlayPause,
        Self::SeekForward,
        Self::SeekBackward,
        Self::NextFrame,
        Self::PreviousFrame,
        Self::Restart,
        Self::PreviousFile,
        Self::NextFile,
        Self::Loop,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
        Self::NextAudioTrack,
        Self::AudioEarlier,
        Self::AudioLater,
        Self::ResetAudioDelay,
        Self::NextSubtitleTrack,
        Self::ToggleSubtitles,
        Self::BiggerSubtitles,
        Self::SmallerSubtitles,
        Self::SubtitlesUp,
        Self::SubtitlesDown,
        Self::SubtitlesEarlier,
        Self::SubtitlesLater,
        Self::ResetSubtitleDelay,
        Self::NextVideoTrack,
        Self::RotateRight,
        Self::RotateLeft,
        Self::FlipHorizontally,
        Self::FlipVertically,
        Self::NextAspectRatio,
        Self::NextCrop,
        Self::ResetPicture,
        Self::FullScreen,
        Self::ExitFullScreen,
        Self::PlayPauseAll,
        Self::SeekForwardAll,
        Self::SeekBackwardAll,
        Self::RestartAll,
        Self::MuteAll,
    ];

    /// Its name in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Self::PlayPause => "play_pause",
            Self::SeekForward => "seek_forward",
            Self::SeekBackward => "seek_backward",
            Self::NextFrame => "next_frame",
            Self::PreviousFrame => "previous_frame",
            Self::Restart => "restart",
            Self::PreviousFile => "previous_file",
            Self::NextFile => "next_file",
            Self::Loop => "loop",
            Self::VolumeUp => "volume_up",
            Self::VolumeDown => "volume_down",
            Self::Mute => "mute",
            Self::NextAudioTrack => "next_audio_track",
            Self::AudioEarlier => "audio_earlier",
            Self::AudioLater => "audio_later",
            Self::ResetAudioDelay => "reset_audio_delay",
            Self::NextSubtitleTrack => "next_subtitle_track",
            Self::NextVideoTrack => "next_video_track",
            Self::ToggleSubtitles => "toggle_subtitles",
            Self::BiggerSubtitles => "bigger_subtitles",
            Self::SmallerSubtitles => "smaller_subtitles",
            Self::SubtitlesUp => "subtitles_up",
            Self::SubtitlesDown => "subtitles_down",
            Self::SubtitlesEarlier => "subtitles_earlier",
            Self::SubtitlesLater => "subtitles_later",
            Self::ResetSubtitleDelay => "reset_subtitle_delay",
            Self::RotateRight => "rotate_right",
            Self::RotateLeft => "rotate_left",
            Self::FlipHorizontally => "flip_horizontally",
            Self::FlipVertically => "flip_vertically",
            Self::NextAspectRatio => "next_aspect_ratio",
            Self::NextCrop => "next_crop",
            Self::ResetPicture => "reset_picture",
            Self::FullScreen => "full_screen",
            Self::ExitFullScreen => "exit_full_screen",
            Self::PlayPauseAll => "play_pause_all",
            Self::RestartAll => "restart_all",
            Self::SeekForwardAll => "seek_forward_all",
            Self::SeekBackwardAll => "seek_backward_all",
            Self::MuteAll => "mute_all",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::PlayPause => "Play/pause",
            Self::SeekForward => "Seek forward",
            Self::SeekBackward => "Seek backward",
            Self::NextFrame => "Next frame",
            Self::PreviousFrame => "Previous frame",
            Self::Restart => "Restart",
            Self::PreviousFile => "Previous file in its folder",
            Self::NextFile => "Next file in its folder",
            Self::Loop => "Loop",
            Self::VolumeUp => "Volume up",
            Self::VolumeDown => "Volume down",
            Self::Mute => "Mute",
            Self::NextAudioTrack => "Next audio track",
            Self::AudioEarlier => "Audio earlier",
            Self::AudioLater => "Audio later",
            Self::ResetAudioDelay => "Reset audio delay",
            Self::NextSubtitleTrack => "Next subtitles",
            Self::NextVideoTrack => "Next video track",
            Self::ToggleSubtitles => "Show/hide subtitles",
            Self::BiggerSubtitles => "Bigger subtitles",
            Self::SmallerSubtitles => "Smaller subtitles",
            Self::SubtitlesUp => "Move subtitles up",
            Self::SubtitlesDown => "Move subtitles down",
            Self::SubtitlesEarlier => "Subtitles earlier",
            Self::SubtitlesLater => "Subtitles later",
            Self::ResetSubtitleDelay => "Reset subtitle delay",
            Self::RotateRight => "Rotate right",
            Self::RotateLeft => "Rotate left",
            Self::FlipHorizontally => "Flip horizontally",
            Self::FlipVertically => "Flip vertically",
            Self::NextAspectRatio => "Next aspect ratio",
            Self::NextCrop => "Next crop",
            Self::ResetPicture => "Reset picture",
            Self::FullScreen => "Full screen",
            Self::ExitFullScreen => "Exit full screen",
            Self::PlayPauseAll => "Play/pause all",
            Self::RestartAll => "Restart all",
            Self::SeekForwardAll => "Seek all forward",
            Self::SeekBackwardAll => "Seek all backward",
            Self::MuteAll => "Mute/unmute all",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Self::PlayPause
            | Self::SeekForward
            | Self::SeekBackward
            | Self::NextFrame
            | Self::PreviousFrame
            | Self::Restart
            | Self::PreviousFile
            | Self::NextFile
            | Self::Loop => Group::Playback,
            Self::VolumeUp
            | Self::VolumeDown
            | Self::Mute
            | Self::NextAudioTrack
            | Self::AudioEarlier
            | Self::AudioLater
            | Self::ResetAudioDelay => Group::Audio,
            Self::NextSubtitleTrack
            | Self::ToggleSubtitles
            | Self::BiggerSubtitles
            | Self::SmallerSubtitles
            | Self::SubtitlesUp
            | Self::SubtitlesDown
            | Self::SubtitlesEarlier
            | Self::SubtitlesLater
            | Self::ResetSubtitleDelay => Group::Subtitles,
            Self::NextVideoTrack
            | Self::RotateRight
            | Self::RotateLeft
            | Self::FlipHorizontally
            | Self::FlipVertically
            | Self::NextAspectRatio
            | Self::NextCrop
            | Self::ResetPicture => Group::Video,
            Self::FullScreen | Self::ExitFullScreen => Group::Window,
            Self::PlayPauseAll
            | Self::RestartAll
            | Self::SeekForwardAll
            | Self::SeekBackwardAll
            | Self::MuteAll => Group::AllFiles,
        }
    }

    pub fn action(self) -> Box<dyn Action> {
        match self {
            Self::PlayPause => Box::new(PlayPause),
            Self::SeekForward => Box::new(SeekForward),
            Self::SeekBackward => Box::new(SeekBackward),
            Self::NextFrame => Box::new(NextFrame),
            Self::PreviousFrame => Box::new(PreviousFrame),
            Self::Restart => Box::new(Restart),
            Self::PreviousFile => Box::new(PreviousFile),
            Self::NextFile => Box::new(NextFile),
            Self::Loop => Box::new(ToggleLoop),
            Self::VolumeUp => Box::new(VolumeUp),
            Self::VolumeDown => Box::new(VolumeDown),
            Self::Mute => Box::new(ToggleMute),
            Self::NextAudioTrack => Box::new(NextAudioTrack),
            Self::AudioEarlier => Box::new(AudioEarlier),
            Self::AudioLater => Box::new(AudioLater),
            Self::ResetAudioDelay => Box::new(ResetAudioDelay),
            Self::NextSubtitleTrack => Box::new(NextSubtitleTrack),
            Self::NextVideoTrack => Box::new(NextVideoTrack),
            Self::ToggleSubtitles => Box::new(ToggleSubtitles),
            Self::BiggerSubtitles => Box::new(BiggerSubtitles),
            Self::SmallerSubtitles => Box::new(SmallerSubtitles),
            Self::SubtitlesUp => Box::new(SubtitlesUp),
            Self::SubtitlesDown => Box::new(SubtitlesDown),
            Self::SubtitlesEarlier => Box::new(SubtitlesEarlier),
            Self::SubtitlesLater => Box::new(SubtitlesLater),
            Self::ResetSubtitleDelay => Box::new(ResetSubtitleDelay),
            Self::RotateRight => Box::new(RotateRight),
            Self::RotateLeft => Box::new(RotateLeft),
            Self::FlipHorizontally => Box::new(FlipHorizontally),
            Self::FlipVertically => Box::new(FlipVertically),
            Self::NextAspectRatio => Box::new(NextAspectRatio),
            Self::NextCrop => Box::new(NextCrop),
            Self::ResetPicture => Box::new(ResetPicture),
            Self::FullScreen => Box::new(ToggleFullScreen),
            Self::ExitFullScreen => Box::new(ExitFullScreen),
            Self::PlayPauseAll => Box::new(PlayPauseAll),
            Self::RestartAll => Box::new(RestartAll),
            Self::SeekForwardAll => Box::new(SeekForwardAll),
            Self::SeekBackwardAll => Box::new(SeekBackwardAll),
            Self::MuteAll => Box::new(ToggleMuteAll),
        }
    }

    /// The keys a command starts out with. `secondary` is ⌘ on macOS and
    /// Ctrl elsewhere.
    fn default_keys(self) -> &'static [&'static str] {
        let mac = cfg!(target_os = "macos");
        match self {
            Self::PlayPause => &["space", "p"],
            Self::SeekForward => &["right"],
            Self::SeekBackward => &["left"],
            Self::NextFrame => &["alt-right", "."],
            Self::PreviousFrame => &["alt-left", ","],
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
            Self::FullScreen if mac => &["ctrl-cmd-f", "f"],
            Self::FullScreen => &["f11", "f"],
            Self::ExitFullScreen => &["escape"],
            // Every file in the window.
            Self::PlayPauseAll => &["shift-space"],
            Self::RestartAll => &["shift-home"],
            Self::SeekForwardAll => &["shift-right"],
            Self::SeekBackwardAll => &["shift-left"],
            Self::MuteAll => &["shift-m"],
        }
    }
}

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

impl Serialize for Hotkeys {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let map: BTreeMap<&str, &Vec<String>> = self
            .0
            .iter()
            .map(|(command, keys)| (command.id(), keys))
            .collect();
        map.serialize(serializer)
    }
}

/// Commands the file doesn't mention keep their defaults, so new ones get
/// theirs; ones it names that no longer exist, and keys that aren't, are
/// left out.
impl<'de> Deserialize<'de> for Hotkeys {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = BTreeMap::<String, Vec<String>>::deserialize(deserializer)?;
        let mut hotkeys = Self::default();
        for (id, keys) in map {
            if let Some(command) = Command::from_id(&id) {
                hotkeys
                    .0
                    .insert(command, keys.iter().filter_map(|k| normalize(k)).collect());
            }
        }
        Ok(hotkeys)
    }
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
