//! The commands, a file for each namespace, in the order they're listed.

mod app;
mod audio;
pub mod block;
pub mod cast;
mod cli;
mod display;
pub mod output;
pub mod playback;
pub mod settings;
mod subtitles;
pub mod video;
pub mod window;

use gpui::Context;
use serde::Deserialize;
use serde_json::Value;

use super::Error;
use super::events::EventKind;
use super::registry::{Args, Outcome, Spec};
use crate::media::tracks::TrackKind;
use crate::ui::player::PlayerView;

/// Every namespace's commands.
pub const ALL: [&[Spec]; 12] = [
    app::COMMANDS,
    window::COMMANDS,
    block::COMMANDS,
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

/// The events of each namespace that has any, in the order of `ALL`.
pub const EVENTS: [(&str, &[EventKind]); 8] = [
    ("app", app::EVENTS),
    ("window", window::EVENTS),
    ("block", block::EVENTS),
    ("playback", playback::EVENTS),
    ("audio", audio::EVENTS),
    ("cast", cast::EVENTS),
    ("output", output::EVENTS),
    ("app.settings", settings::EVENTS),
];

/// What opens files in a window, other than a command: events' cause.
const DROPPED: &str =
    "files dropped on a window or picked when `window.open_files` or `window.open_url` asks";

/// The namespaces, in the order Settings → Hotkeys lists their commands,
/// each with its title there.
pub const NAMESPACES: [(&str, &str); 11] = [
    ("playback", "Playback"),
    ("audio", "Audio"),
    ("subtitles", "Subtitles"),
    ("video", "Video"),
    ("window", "Windows"),
    ("block", "Blocks in a window"),
    ("cast", "Casting"),
    ("output", "Audio Output"),
    ("app.settings", "Settings"),
    ("app", "Plays"),
    ("cli", "Command line"),
];

/// What shows or hides a panel over the video, other than a command:
/// events' cause.
const PANELS_SET: &str = "Settings turning a panel over the video on or off, importing a \
     settings file or resetting every setting";

/// The panels over the video an event says are shown.
const VISIBLE: &str = "Those shown, of `controls`, `top_bar`, `track_switcher` and `notes`.";

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
