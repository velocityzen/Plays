//! A tile's playlist: files opened at once, played one after another in
//! it, as Settings → Playback can have them; and which of them plays.

use crate::media::Source;

pub struct Playlist {
    sources: Vec<Source>,
    /// Where the one playing is in it.
    at: usize,
}

impl Playlist {
    /// `sources`, to play one after another from the first; none for
    /// fewer than two.
    pub fn new(sources: Vec<Source>) -> Option<Self> {
        (sources.len() > 1).then_some(Self { sources, at: 0 })
    }

    /// The one before (`-1`) or after (`1`) the one playing, if there is.
    pub fn beside(&self, direction: i32) -> Option<&Source> {
        self.sources.get(self.beside_at(direction)?)
    }

    fn beside_at(&self, direction: i32) -> Option<usize> {
        if direction < 0 {
            self.at.checked_sub(1)
        } else {
            Some(self.at + 1)
        }
    }

    /// Move on to the one before (`-1`) or after (`1`), if there is, to
    /// play it.
    pub fn step(&mut self, direction: i32) -> Option<Source> {
        let at = self.beside_at(direction)?;
        let source = self.sources.get(at)?.clone();
        self.at = at;
        Some(source)
    }

    /// Play `source` from now on, as it's in it: where it first is.
    /// Whether it's in it.
    pub fn go_to(&mut self, source: &Source) -> bool {
        match self.sources.iter().position(|s| s == source) {
            Some(at) => {
                self.at = at;
                true
            }
            None => false,
        }
    }

    /// Where the one playing is, as its tile's header says: "2 of 5".
    pub fn counter(&self) -> String {
        format!("{} of {}", self.at + 1, self.sources.len())
    }
}

#[cfg(test)]
#[path = "tests/playlist.rs"]
mod tests;
