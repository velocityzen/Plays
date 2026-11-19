use std::{fmt, io};

use crate::soap::Fault;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The device answered with an HTTP error, not UPnP's.
    Http {
        request: String,
        status: u16,
    },
    /// The device refused an action, saying why as UPnP does.
    Upnp {
        action: String,
        fault: Fault,
        /// UPnP's number for it, which `fault` names.
        code: u16,
        description: String,
    },
    /// The device said something that isn't UPnP.
    Protocol(String),
    /// The device hasn't the service this needs.
    Unsupported(&'static str),
    /// The session is over: the device went away, or something else is
    /// playing.
    Ended,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn protocol(what: impl Into<String>) -> Self {
        Self::Protocol(what.into())
    }

    /// Whether the device seems to be gone, rather than refusing.
    pub(crate) fn is_unreachable(&self) -> bool {
        matches!(self, Self::Io(_))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Http { request, status } => write!(f, "{request}: HTTP {status}"),
            Self::Upnp {
                action,
                code,
                description,
                ..
            } => write!(f, "{action} refused: {code} {description}"),
            Self::Protocol(what) => write!(f, "not UPnP: {what}"),
            Self::Unsupported(service) => write!(f, "the device has no {service}"),
            Self::Ended => write!(f, "the session is over"),
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
