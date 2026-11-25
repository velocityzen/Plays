use std::path::PathBuf;
use std::{fmt, io};

/// Why an update couldn't be checked for, downloaded or installed. Each
/// says so as a phrase, to follow "can't update: ".
#[derive(Debug)]
pub enum Error {
    /// Reading or writing a file here.
    Io(io::Error),
    /// The server answered with an HTTP error.
    Http { url: String, status: u16 },
    /// The server couldn't be reached, or stopped answering.
    Network { url: String, message: String },
    /// A URL that isn't HTTPS, or HTTP to this machine.
    Insecure(String),
    /// More came than the feed, or the bounds for a feed, allow.
    TooBig { url: String, limit: u64 },
    /// A signature in the feed doesn't hold, or no key the app has made it.
    Signature(String),
    /// The feed isn't one, or isn't this app's: why.
    Feed(String),
    /// The feed's time is up, and no newer one is served.
    Expired,
    /// A download, or a file it makes, isn't what the feed or the update
    /// says it is.
    Mismatch(String),
    /// The update file isn't one, or doesn't apply to the install: why.
    Update(String),
    /// Where the app runs from isn't its own to update.
    NotInstalled(NotInstalled),
    /// The folder the app is installed in can't be written to.
    Permission(PathBuf),
    /// macOS: the update's code signature doesn't hold, or isn't the
    /// running app's team's.
    CodeSignature(String),
    /// Stopped before it was done.
    Cancelled,
}

/// Why the app, where it runs from, isn't its own to update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotInstalled {
    /// It runs from a Cargo `target` folder.
    BuiltFromSource,
    /// macOS: it isn't in an app bundle.
    NotAnApp,
    /// macOS runs it from a copy of its own (App Translocation), as it
    /// hasn't been moved since it was downloaded.
    Translocated,
    /// A package manager or store installed it, and updates it: its name.
    Managed(&'static str),
    /// Its executable couldn't be found.
    NoExecutable,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Whether trying again later may work: the network, or the server.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Network { .. }
                | Self::Http { status: 500.., .. }
                | Self::Http { status: 429, .. }
        )
    }

    pub(crate) fn update(why: impl Into<String>) -> Self {
        Self::Update(why.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Http { url, status } => write!(f, "{url} answered HTTP {status}"),
            Self::Network { url, message } => write!(f, "{url}: {message}"),
            Self::Insecure(url) => write!(f, "{url} isn't HTTPS"),
            Self::TooBig { url, limit } => write!(f, "{url} is over {limit} bytes"),
            Self::Signature(why) => write!(f, "a signature in the feed doesn't hold: {why}"),
            Self::Feed(why) => write!(f, "the feed isn't one: {why}"),
            Self::Expired => write!(f, "the feed is out of date"),
            Self::Mismatch(what) => write!(f, "{what} isn't what the feed says"),
            Self::Update(why) => write!(f, "the update doesn't apply: {why}"),
            Self::NotInstalled(why) => write!(f, "{why}"),
            Self::Permission(folder) => write!(f, "{} can't be written to", folder.display()),
            Self::CodeSignature(why) => write!(f, "the update's code signature: {why}"),
            Self::Cancelled => write!(f, "it was stopped"),
        }
    }
}

impl fmt::Display for NotInstalled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuiltFromSource => write!(f, "it was built from source"),
            Self::NotAnApp => write!(f, "it isn't running as an app"),
            Self::Translocated => write!(
                f,
                "it runs where it was downloaded: move it to Applications"
            ),
            Self::Managed(by) => write!(f, "{by} installed it, and updates it"),
            Self::NoExecutable => write!(f, "where it runs from can't be told"),
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

impl From<NotInstalled> for Error {
    fn from(why: NotInstalled) -> Self {
        Self::NotInstalled(why)
    }
}
