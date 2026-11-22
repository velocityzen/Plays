//! What the command line takes, as clap reads it: its subcommands, and
//! the options every one takes.

use std::ffi::OsString;

use clap::{Args, Parser, Subcommand};

/// Plays from a terminal.
#[derive(Debug, Parser)]
#[command(
    name = "plays",
    version,
    about = "Drive Plays from a terminal: open files, run its commands, and hear what \
             changes.",
    long_about = "Drive Plays from a terminal: open files, run its commands, and hear \
                  what changes. Each asks the Plays running, starting it if none is.\n\n\
                  plays FILE… opens files, folders and streams in Plays, as opening \
                  them from the Finder does.",
    after_help = "Exit statuses: 0 done, 1 failed, 2 a bad command line, 3 nothing by \
                  the name given, 4 no Plays to ask, 5 control from the command line \
                  turned off (Settings → Command Line turns it on), 130 stopped with \
                  Ctrl-C."
)]
pub struct Cli {
    #[command(flatten)]
    pub options: Options,
    #[command(subcommand)]
    pub command: Command,
}

/// What every subcommand takes.
#[derive(Clone, Debug, Default, Args)]
pub struct Options {
    /// Print what comes back as JSON.
    #[arg(long, global = true)]
    pub json: bool,
    /// Don't start Plays if it isn't running.
    #[arg(long, global = true)]
    pub no_autostart: bool,
    /// The window to act on: its id, its label, its number from 1, or the end
    /// of its id. The one in front by default.
    #[arg(short, long, global = true, value_name = "WINDOW")]
    pub window: Option<String>,
    /// The tile to act on: its id, its label, its place in the window from 1,
    /// or the end of its id. The one keys go to by default.
    #[arg(short, long, global = true, value_name = "TILE")]
    pub tile: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a command: plays do playback.seek time=90.
    Do {
        /// The command's id, as plays actions lists them.
        command: String,
        /// Its arguments, each KEY=VALUE. A key given again makes a list.
        #[arg(value_name = "KEY=VALUE")]
        args: Vec<String>,
    },
    /// List the commands: their ids, titles and keys.
    Actions {
        /// Write the command reference, in Markdown.
        #[arg(long)]
        markdown: bool,
    },
    /// List, describe and call the API's commands, with JSON arguments.
    #[command(subcommand)]
    Api(Api),
    /// List the windows, each with its tiles.
    Ls,
    /// List, inspect, go to, open, close, rename and move windows.
    #[command(subcommand)]
    Window(Window),
    /// List, inspect, go to, close, maximize, move and rename tiles.
    #[command(subcommand)]
    Tile(Tile),
    /// Open files, folders and streams, in the window in front or the one
    /// named.
    Open {
        #[arg(required = true, value_name = "FILE|URL")]
        paths: Vec<OsString>,
        /// In a new window.
        #[arg(long)]
        new_window: bool,
    },
    /// Print what changes, as it does: the app's, the window's named (-w), or
    /// everything's (--all).
    Events {
        /// Everything's: every window's and tile's too.
        #[arg(long)]
        all: bool,
        /// Each tile's position too, once a second.
        #[arg(long)]
        positions: bool,
        /// Stop after this many.
        #[arg(long, value_name = "N")]
        count: Option<usize>,
    },
    /// Wait for an event from the tile (-t) or window (-w) named, such as
    /// ended, and print it.
    Wait {
        /// The event's type, as plays events prints it: ended, paused,
        /// tile_closed…
        event: String,
        /// Give up after this many seconds, with exit status 1.
        // So -1 is read as a time, and refused as one, not as an option.
        #[arg(long, value_name = "SECONDS", allow_negative_numbers = true)]
        timeout: Option<f64>,
    },
    /// List, read and change settings.
    #[command(subcommand)]
    Settings(Settings),
    /// List the commands keys run, with the keys they have now.
    Keymap,
    /// Print this program's version, and that of the Plays running.
    Version,
}

#[derive(Debug, Subcommand)]
pub enum Api {
    /// Describe every command, as the Plays running has them.
    List,
    /// Describe a command: its arguments, keys and what it hands back.
    Describe { command: String },
    /// Run a command with its arguments as a JSON object, and print what it
    /// hands back as JSON.
    Call {
        command: String,
        #[arg(value_name = "JSON")]
        args: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum Window {
    /// List the windows, in the order ⌘1 to ⌘9 number them.
    Ls,
    /// Say all about a window.
    Inspect { window: Option<String> },
    /// Bring a window to the front: one named, or next or prev.
    Focus { window: Option<String> },
    /// Open an empty window, labeled if a label is given, and print its id.
    New { label: Option<String> },
    /// Close windows: those named, or the one in front.
    Close { windows: Vec<String> },
    /// Label a window: [WINDOW] LABEL. A blank label takes it away.
    Rename {
        #[arg(num_args = 1..=2, required = true, value_names = ["WINDOW", "LABEL"])]
        names: Vec<String>,
    },
    /// Give a window another number.
    Move {
        window: Option<String>,
        #[command(flatten)]
        to: Place,
    },
}

#[derive(Debug, Subcommand)]
pub enum Tile {
    /// List the window's tiles, in the grid's order.
    Ls,
    /// Say all about a tile.
    Inspect { tile: Option<String> },
    /// Put a tile in front, keys going to it, and its window too.
    Focus { tile: Option<String> },
    /// Close tiles: those named, or the one keys go to.
    Close { tiles: Vec<String> },
    /// Fill the window with a tile, or show every tile again.
    Zoom { tile: Option<String> },
    /// Put a tile in another place in its window's grid.
    Move {
        tile: Option<String>,
        #[command(flatten)]
        to: Place,
    },
    /// Label a tile: [TILE] LABEL. A blank label takes it away.
    Rename {
        #[arg(num_args = 1..=2, required = true, value_names = ["TILE", "LABEL"])]
        names: Vec<String>,
    },
}

/// Where a window or tile moves to.
#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct Place {
    /// Before the one named.
    #[arg(long, value_name = "NAME")]
    pub before: Option<String>,
    /// After the one named.
    #[arg(long, value_name = "NAME")]
    pub after: Option<String>,
    /// At this place, from 1.
    #[arg(long, value_name = "N")]
    pub index: Option<usize>,
}

#[derive(Debug, Subcommand)]
pub enum Settings {
    /// List every setting, with its value.
    Ls,
    /// Read a setting by its key, such as subtitles.size.
    Get { key: String },
    /// Change a setting: VALUE as the settings file keeps it, true, 0.5 or
    /// dark.
    Set { key: String, value: String },
}
