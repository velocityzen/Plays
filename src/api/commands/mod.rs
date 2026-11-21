//! The commands, a file for each namespace, in the order they're listed.

mod app;
mod audio;
pub mod cast;
pub mod output;
pub mod playback;
mod subtitles;
mod tile;
pub mod video;
pub mod window;

use serde_json::Value;

use super::registry::{Outcome, Spec};

/// Every namespace's commands.
pub const ALL: [&[Spec]; 9] = [
    app::COMMANDS,
    window::COMMANDS,
    tile::COMMANDS,
    playback::COMMANDS,
    audio::COMMANDS,
    subtitles::COMMANDS,
    video::COMMANDS,
    cast::COMMANDS,
    output::COMMANDS,
];

/// The namespaces, in the order Settings → Hotkeys lists their commands,
/// each with its title there.
pub const NAMESPACES: [(&str, &str); 9] = [
    ("playback", "Playback"),
    ("audio", "Audio"),
    ("subtitles", "Subtitles"),
    ("video", "Video"),
    ("window", "Windows"),
    ("tile", "Tiles in a window"),
    ("cast", "Casting"),
    ("output", "Output"),
    ("app", "Plays"),
];

/// Done, with nothing to hand back.
fn done() -> Outcome {
    Ok(Value::Null)
}
