//! The commands, a file for each namespace, in the order they're listed.

mod app;
mod audio;
mod cast;
mod output;
mod playback;
mod subtitles;
mod tile;
mod video;
mod window;

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

/// Done, with nothing to hand back.
fn done() -> Outcome {
    Ok(Value::Null)
}
