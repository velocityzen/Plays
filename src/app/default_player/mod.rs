//! Plays as the default player: the app that opens each format's files
//! now, and Plays made to open the formats picked in Settings → Default
//! Player. From macOS 26.4, macOS asks the user to confirm each one, which
//! is why they're picked and not all set at once; Linux's `xdg-mime`
//! asks nothing.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(any(target_os = "linux", test))]
mod xdg;

use file_types::Format;
use gpui::{App, Global, Task};

/// A format Plays opens here, and the app that opens its files now.
#[derive(Clone, Debug, PartialEq)]
pub struct Opener {
    pub format: &'static Format,
    /// The app, by name; none when no app does.
    pub app: Option<String>,
    /// Whether Plays does, whichever type its files have.
    pub plays: bool,
}

impl Opener {
    /// Who opens its files, for Settings: `VLC opens them`.
    pub fn says(&self) -> String {
        match (&self.app, self.plays) {
            (_, true) => "Plays opens them".to_owned(),
            (Some(app), false) => format!("{app} opens them"),
            (None, false) => "No app opens them".to_owned(),
        }
    }
}

/// Each format Plays opens here, and the app that opens it now; or why
/// that can't be told.
pub type Openers = Result<Vec<Opener>, String>;

/// What asking for Plays to open some formats came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Made {
    /// The formats Plays opens now.
    pub set: Vec<&'static Format>,
    /// The formats it still doesn't, and why.
    pub not_set: Vec<(&'static Format, String)>,
}

impl Made {
    /// What it came to, for Settings: how many of the formats picked Plays
    /// opens now, then each it doesn't, by its extension too, as two may
    /// share a name, and why. The pane lists each format with its app, so
    /// those set go unnamed.
    pub fn says(&self) -> String {
        let (set, picked) = (self.set.len(), self.set.len() + self.not_set.len());
        let mut said = Vec::new();
        match set {
            0 => {}
            1 if picked == 1 => said.push("Plays opens the format picked now.".to_owned()),
            set if set == picked => {
                said.push(format!("Plays opens the {picked} formats picked now."));
            }
            set => said.push(format!(
                "Plays opens {set} of the {picked} formats picked now."
            )),
        }
        for (format, why) in &self.not_set {
            said.push(format!(
                "Not {} (.{}): {why}.",
                format.name,
                format.extension()
            ));
        }
        said.join(" ")
    }
}

/// How the defaults are read and set: the system's own, or a test's.
#[derive(Clone, Copy)]
pub struct System {
    /// Why Plays can't be made the default player here, if it can't.
    pub cant: fn() -> Option<String>,
    /// Each format Plays opens here, and the app that opens it now.
    pub read: fn(&mut App) -> Task<Openers>,
    /// Plays made to open `formats`, as far as the system, and on macOS
    /// the user, let it.
    pub make: fn(Vec<&'static Format>, &mut App) -> Task<Made>,
}

impl Global for System {}

impl System {
    /// The one in use: a test's, or the system's own.
    pub fn get(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or(OWN)
    }
}

#[cfg(target_os = "macos")]
const OWN: System = System {
    cant: mac::cant,
    read: mac::read,
    make: mac::make,
};

#[cfg(target_os = "linux")]
const OWN: System = System {
    cant: xdg::cant,
    read: xdg::read,
    make: xdg::make,
};

#[cfg(test)]
#[path = "../tests/default_player.rs"]
mod tests;
