//! The keys commands take, which Settings can change. Each command that
//! takes keys starts out with its own (`api::Spec::keys`); the settings
//! file keeps those changed. Keys are stored as gpui keystrokes, such as
//! `cmd-shift-l`, and bound to the command's `Run`, where the command's
//! keys work (`api::Reach`).

use std::collections::BTreeMap;

use gpui::{DummyKeyboardMapper, KeyBinding, KeyBindingContextPredicate, Keystroke};
use serde::{Deserialize, Serialize};

use crate::api::{self, Reach, Run, Spec};

/// The key context a player window's keys work in: a window with files
/// open, but not while typing in a text field there (the stream URL), so
/// Space and letters type.
pub const CONTEXT: &str = "Workspace && !TextInput";

/// Where `key` works, given to a command whose keys reach `reach`. A key
/// that types (one without ⌘ or Ctrl) doesn't work while typing, and one
/// that works anywhere doesn't while Settings waits for keys to record
/// (its key context then has `Recording`), so it can take them.
fn context(reach: Reach, key: &Keystroke) -> &'static str {
    let types = !(key.modifiers.platform || key.modifiers.control);
    match (reach, types) {
        (Reach::NotTyping, _) | (Reach::Files, true) => CONTEXT,
        (Reach::Files, false) => "Workspace",
        (Reach::Anywhere, true) => "!TextInput && !Recording",
        (Reach::Anywhere, false) => "!Recording",
    }
}

/// Whether a command's keys work only in a player window with files open.
/// A key goes to one such command, and one other: the player window's
/// wins there, as ⌘W closes the file in front there and the window in
/// front elsewhere.
fn in_players(reach: Reach) -> bool {
    reach != Reach::Anywhere
}

/// The commands that take keys, in the order they're listed.
pub fn commands() -> impl Iterator<Item = &'static Spec> {
    api::all().filter(|spec| spec.takes_keys())
}

/// Keys commands started out with before they had the ones they have now.
/// A settings file that still has them gets today's instead.
const EARLIER_DEFAULTS: &[(&str, &[&str])] = &[
    ("playback.next_frame", &["alt-right", "."]),
    ("playback.previous_frame", &["alt-left", ","]),
];

/// The keys of every command that takes them, by its id.
#[derive(Clone, Debug, PartialEq)]
pub struct Hotkeys(BTreeMap<&'static str, Vec<String>>);

impl Default for Hotkeys {
    fn default() -> Self {
        Self(
            commands()
                .map(|spec| (spec.id, default_keys(spec)))
                .collect(),
        )
    }
}

fn default_keys(spec: &Spec) -> Vec<String> {
    spec.keys.iter().filter_map(|key| normalize(key)).collect()
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
    /// The keys of `command`, by its id.
    pub fn keys(&self, command: &str) -> &[String] {
        self.0.get(command).map_or(&[], Vec::as_slice)
    }

    /// Give `command` the key, taking it from the command that had it
    /// where `command`'s keys work, which is returned. One that takes no
    /// keys gets none.
    pub fn add(&mut self, command: &str, key: &str) -> Option<&'static str> {
        let key = normalize(key)?;
        let spec = api::find(command).filter(|spec| self.0.contains_key(spec.id))?;
        let clashes = |other: &str| {
            api::find(other).is_some_and(|other| in_players(other.reach) == in_players(spec.reach))
        };
        let mut previous = None;
        for (other, keys) in &mut self.0 {
            if *other != spec.id && clashes(other) && keys.contains(&key) {
                keys.retain(|k| *k != key);
                previous = previous.or(Some(*other));
            }
        }
        let keys = self.0.entry(spec.id).or_default();
        keys.retain(|k| *k != key);
        keys.push(key);
        previous
    }

    pub fn remove(&mut self, command: &str, key: &str) {
        if let Some(keys) = self.0.get_mut(command) {
            keys.retain(|k| k != key);
        }
    }

    /// Key bindings for gpui's keymap: each key to its command's `Run`.
    /// Those of commands that work in player windows come last, so where
    /// a key is also another's that works anywhere, theirs wins there.
    pub fn bindings(&self) -> Vec<KeyBinding> {
        let mut commands: Vec<(&Spec, &Vec<String>)> = self
            .0
            .iter()
            .filter_map(|(command, keys)| Some((api::find(command)?, keys)))
            .collect();
        commands.sort_by_key(|(spec, _)| in_players(spec.reach));
        commands
            .into_iter()
            .flat_map(|(spec, keys)| keys.iter().map(move |key| (spec, key)))
            .filter_map(|(spec, key)| {
                let keystroke = Keystroke::parse(key).ok()?;
                let context = KeyBindingContextPredicate::parse(context(spec.reach, &keystroke))
                    .ok()
                    .map(Into::into);
                KeyBinding::load(
                    key,
                    Box::new(Run::command(spec.id)),
                    context,
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
/// defaults as they come; each under its name in the settings file.
impl Serialize for Hotkeys {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let map: BTreeMap<&str, &Vec<String>> = commands()
            .filter_map(|spec| Some((spec, self.0.get(spec.id)?)))
            .filter(|(spec, keys)| **keys != default_keys(spec))
            .map(|(spec, keys)| (spec.setting_name(), keys))
            .collect();
        map.serialize(serializer)
    }
}

/// Commands the file doesn't mention keep their defaults, so new ones get
/// theirs, as do ones it gives keys they no longer start out with; ones it
/// names that no longer exist, and keys that aren't, are left out. A
/// command is named by its name in the settings file, or by its id.
impl<'de> Deserialize<'de> for Hotkeys {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = BTreeMap::<String, Vec<String>>::deserialize(deserializer)?;
        let mut hotkeys = Self::default();
        for (name, keys) in map {
            let named = commands().find(|spec| spec.setting_name() == name || spec.id == name);
            if let Some(spec) = named {
                let keys: Vec<String> = keys.iter().filter_map(|k| normalize(k)).collect();
                if !is_earlier_default(spec.id, &keys) {
                    hotkeys.0.insert(spec.id, keys);
                }
            }
        }
        Ok(hotkeys)
    }
}

fn is_earlier_default(command: &str, keys: &[String]) -> bool {
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
#[path = "tests/hotkeys.rs"]
mod tests;
