//! What can go wrong running a command, each with the exit status the
//! command line gives it.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a command didn't run, or didn't do what it was asked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    /// There's no command by that id.
    NoSuchCommand { command: String },
    /// Its arguments, or the window or tile named, aren't ones it takes.
    BadArgs { command: String, message: String },
    /// Nothing goes by that name: a window, a tile, a device.
    NotFound { what: String, name: String },
    /// More than one thing goes by that name; `matches` are their ids.
    Ambiguous {
        what: String,
        name: String,
        matches: Vec<String>,
    },
    /// It ran, and couldn't do it.
    Failed { message: String },
    /// Plays doesn't take it from where it came: from a command line, while
    /// Settings has control from there off.
    NotAllowed { message: String },
}

impl Error {
    pub fn bad_args(command: &str, message: impl Into<String>) -> Self {
        Self::BadArgs {
            command: command.to_owned(),
            message: message.into(),
        }
    }

    pub fn not_found(what: &str, name: impl Into<String>) -> Self {
        Self::NotFound {
            what: what.to_owned(),
            name: name.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            message: message.into(),
        }
    }

    /// A command line asked, and Settings has control from there off.
    pub fn command_line_off() -> Self {
        Self::NotAllowed {
            message: "control from the command line is off: turn it on in Settings → \
                      Command Line"
                .to_owned(),
        }
    }

    /// The exit status the command line gives it, as Rex's does: 1 for
    /// no such command or failed, 2 for bad arguments, 3 for a target
    /// not found or ambiguous; and 5 for control from the command line
    /// turned off.
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::NoSuchCommand { .. } | Self::Failed { .. } => 1,
            Self::BadArgs { .. } => 2,
            Self::NotFound { .. } | Self::Ambiguous { .. } => 3,
            Self::NotAllowed { .. } => 5,
        }
    }

    /// Whether nothing was there to run it on, as with a key pressed in an
    /// empty window: nothing to warn of.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchCommand { command } => write!(f, "no command {command}"),
            Self::BadArgs { command, message } => write!(f, "{command}: {message}"),
            Self::NotFound { what, name } => write!(f, "no {what} {name}"),
            Self::Ambiguous {
                what,
                name,
                matches,
            } => write!(f, "{name} could be the {what} {}", matches.join(" or ")),
            Self::Failed { message } | Self::NotAllowed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
#[path = "tests/error.rs"]
mod tests;
