//! The commands, a file for each namespace, in the order they're listed.

mod app;
mod audio;
pub mod cast;
mod cli;
mod display;
pub mod output;
pub mod playback;
pub mod settings;
mod subtitles;
pub mod tile;
pub mod video;
mod view;
pub mod window;

use gpui::Context;
use serde::Deserialize;
use serde_json::Value;

use super::Error;
use super::registry::{Args, Outcome, Spec};
use crate::media::tracks::TrackKind;
use crate::ui::player::PlayerView;

/// Every namespace's commands.
pub const ALL: [&[Spec]; 13] = [
    app::COMMANDS,
    window::COMMANDS,
    view::COMMANDS,
    tile::COMMANDS,
    playback::COMMANDS,
    audio::COMMANDS,
    subtitles::COMMANDS,
    video::COMMANDS,
    cast::COMMANDS,
    output::COMMANDS,
    display::COMMANDS,
    settings::COMMANDS,
    cli::COMMANDS,
];

/// The namespaces, in the order Settings → Hotkeys lists their commands,
/// each with its title there.
pub const NAMESPACES: [(&str, &str); 12] = [
    ("playback", "Playback"),
    ("audio", "Audio"),
    ("subtitles", "Subtitles"),
    ("video", "Video"),
    ("view", "Over the video"),
    ("window", "Windows"),
    ("tile", "Tiles in a window"),
    ("cast", "Casting"),
    ("output", "Output"),
    ("settings", "Settings"),
    ("app", "Plays"),
    ("cli", "Command line"),
];

/// Done, with nothing to hand back.
fn done() -> Outcome {
    Ok(Value::Null)
}

/// Play the track of `kind` numbered in `args`, from 1 as the tracks menu
/// lists them; with none, no subtitles.
fn select_track(
    args: Args,
    kind: TrackKind,
    player: &mut PlayerView,
    cx: &mut Context<PlayerView>,
) -> Outcome {
    #[derive(Deserialize)]
    struct Track {
        number: Option<usize>,
    }
    let Track { number } = args.read()?;
    if player.select_track_number(kind, number, cx) {
        done()
    } else {
        let number = number.map_or_else(|| "off".to_owned(), |number| number.to_string());
        Err(Error::not_found("track", number))
    }
}
