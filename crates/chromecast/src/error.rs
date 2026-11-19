use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The device didn't answer in time.
    Timeout(String),
    /// The device said no: what was asked, and what it said.
    Refused {
        request: String,
        reason: String,
    },
    /// The device said something that isn't Cast.
    Protocol(String),
    /// The session is over: the device went away, or another sender took
    /// it over.
    Ended,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn protocol(what: impl Into<String>) -> Self {
        Self::Protocol(what.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Timeout(request) => write!(f, "no answer to {request}"),
            Self::Refused { request, reason } => write!(f, "{request} refused: {reason}"),
            Self::Protocol(what) => write!(f, "not Cast: {what}"),
            Self::Ended => write!(f, "the cast is over"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}
