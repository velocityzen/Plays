use std::fmt;
use std::io;

/// What went wrong talking to a receiver.
#[derive(Debug)]
pub enum Error {
    /// It couldn't be reached, or the connection broke.
    Io(io::Error),
    /// It answered a request with this status.
    Status { request: String, status: u16 },
    /// It wants to be paired with a PIN first.
    NeedsPin,
    /// Pairing failed: a wrong PIN, or the receiver turned it down for
    /// now, busy or asking to wait.
    Pairing(String),
    /// It no longer takes the pairing it was asked to verify, as after
    /// it's reset, or it isn't the receiver paired with: paired again with
    /// its PIN, it plays.
    Forgotten(String),
    /// It turned this computer away before pairing (`403 Forbidden`): it
    /// lets only some senders play to it, as its settings say, such as a
    /// Mac's own user's devices or a home's people.
    Refused,
    /// It said something unexpected.
    Protocol(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn protocol(what: impl Into<String>) -> Self {
        Self::Protocol(what.into())
    }

    pub(crate) fn pairing(what: impl Into<String>) -> Self {
        Self::Pairing(what.into())
    }

    pub(crate) fn forgotten(what: impl Into<String>) -> Self {
        Self::Forgotten(what.into())
    }

    /// The receiver is gone: off, or off the network.
    pub fn is_gone(&self) -> bool {
        matches!(self, Self::Io(_))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Status { request, status } => write!(f, "{request} was answered {status}"),
            Self::NeedsPin => f.write_str("it needs to be paired with the PIN it shows"),
            Self::Pairing(why) => write!(f, "pairing failed: {why}"),
            Self::Forgotten(why) => write!(f, "to be paired again: {why}"),
            Self::Refused => f.write_str("it doesn't let this computer play to it"),
            Self::Protocol(why) => f.write_str(why),
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
