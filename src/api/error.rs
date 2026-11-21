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

    /// The exit status the command line gives it, as Rex's does: 1 for
    /// no such command or failed, 2 for bad arguments, 3 for a target
    /// not found or ambiguous.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the command line's, once there is one")
    )]
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::NoSuchCommand { .. } | Self::Failed { .. } => 1,
            Self::BadArgs { .. } => 2,
            Self::NotFound { .. } | Self::Ambiguous { .. } => 3,
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
            Self::Failed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_has_its_exit_status_and_says_what_went_wrong() {
        let cases = [
            (
                Error::NoSuchCommand {
                    command: "dance".into(),
                },
                1,
                "no command dance",
            ),
            (
                Error::bad_args("playback.go_to_chapter", "needs number"),
                2,
                "playback.go_to_chapter: needs number",
            ),
            (Error::not_found("window", "9"), 3, "no window 9"),
            (
                Error::Ambiguous {
                    what: "tile".into(),
                    name: "a1".into(),
                    matches: vec!["0000ffa1".into(), "1234aaa1".into()],
                },
                3,
                "a1 could be the tile 0000ffa1 or 1234aaa1",
            ),
            (Error::failed("the window closed"), 1, "the window closed"),
        ];
        for (error, status, message) in cases {
            assert_eq!(error.exit_status(), status, "{error:?}");
            assert_eq!(error.to_string(), message);
        }
        assert!(Error::not_found("tile", "x").is_not_found());
        assert!(!Error::failed("x").is_not_found());
    }

    #[test]
    fn says_which_kind_it_is_as_json() {
        let json = serde_json::to_value(Error::not_found("window", "9")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"error": "not_found", "what": "window", "name": "9"})
        );
        assert_eq!(
            serde_json::from_value::<Error>(json).unwrap(),
            Error::not_found("window", "9")
        );
    }
}
