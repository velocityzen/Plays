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

use gpui::{App, Context};
use serde::Deserialize;
use serde_json::Value;

use super::Error;
use super::events::EventKind;
use super::registry::{Args, Outcome, Spec};
use crate::media::tracks::TrackKind;
use crate::ui::player::PlayerView;
use crate::ui::workspace::all::Players;

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
pub const EVENTS: [(&str, &[EventKind]); 9] = [
    ("app", app::EVENTS),
    ("window", window::EVENTS),
    ("block", block::EVENTS),
    ("block.playback", playback::EVENTS),
    ("block.audio", audio::EVENTS),
    ("block.subtitles", subtitles::EVENTS),
    ("cast", cast::EVENTS),
    ("output", output::EVENTS),
    ("app.settings", settings::EVENTS),
];

/// What opens files in a window, other than a command: events' cause.
const DROPPED: &str =
    "files dropped on a window or picked when `window.open_files` or `window.open_url` asks";

/// The namespaces, in the order Settings → Hotkeys lists their commands,
/// each with its title there.
pub const NAMESPACES: [(&str, &str); 15] = [
    ("block.playback", "Playback"),
    ("window.playback", "Playback in a window"),
    ("app.playback", "Playback in every window"),
    ("block.audio", "Audio"),
    ("window.audio", "Audio in a window"),
    ("app.audio", "Audio in every window"),
    ("block.subtitles", "Subtitles"),
    ("block.video", "Video"),
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

/// What a feature's commands do with it: `<feature>.toggle`, `.on` and
/// `.off`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    /// On if it's off, else off.
    Toggle,
    /// On, unless it is already.
    On,
    /// Off, unless it is already.
    Off,
}

impl Switch {
    /// Whether it's to be on, given whether it is.
    pub fn turn(self, on: bool) -> bool {
        match self {
            Self::Toggle => !on,
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// The panels over the video, each as a `Panel` picks it.
mod panels {
    use crate::settings::OverVideo;

    /// The seek bar and the buttons at the bottom.
    pub fn controls(shown: &mut OverVideo) -> &mut bool {
        &mut shown.controls
    }

    /// The file's name and the buttons at the top.
    pub fn top_bar(shown: &mut OverVideo) -> &mut bool {
        &mut shown.top_bar
    }

    /// The reel of tracks a shortcut rolls to.
    pub fn track_switcher(shown: &mut OverVideo) -> &mut bool {
        &mut shown.track_switcher
    }

    /// Notes on what a shortcut changed.
    pub fn notes(shown: &mut OverVideo) -> &mut bool {
        &mut shown.notes
    }
}

/// Done, with nothing to hand back.
fn done() -> Outcome {
    Ok(Value::Null)
}

/// A track's number, from 1 as the tracks menu lists them; none for no
/// subtitles.
#[derive(Deserialize)]
struct Track {
    number: Option<usize>,
}

/// That a file has no track numbered `number`.
fn no_track(number: Option<usize>) -> Error {
    let number = number.map_or_else(|| "off".to_owned(), |number| number.to_string());
    Error::not_found("track", number)
}

/// Play the track of `kind` numbered in `args`, from 1 as the tracks menu
/// lists them; with none, no subtitles.
fn select_track(
    args: Args,
    kind: TrackKind,
    player: &mut PlayerView,
    cx: &mut Context<PlayerView>,
) -> Outcome {
    let Track { number } = args.read()?;
    if player.select_track_number(kind, number, cx) {
        done()
    } else {
        Err(no_track(number))
    }
}

/// `select_track` in each block that has the track.
fn select_tracks(args: Args, kind: TrackKind, players: &Players, cx: &mut App) -> Outcome {
    let Track { number } = args.read()?;
    if players.select_track_number(kind, number, cx) {
        done()
    } else {
        Err(no_track(number))
    }
}
