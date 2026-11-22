//! Which tracks play as a file opens: the ones picked in it before, as
//! `progress` keeps them with where it was left; else those in the first
//! language Settings prefer that it has; else the ones it wants played.

use serde::{Deserialize, Serialize};

use super::tracks::{Track, TrackKind, same_language};

/// The tracks picked in a file, to play again as it opens again. Video
/// isn't kept: a file seldom has more than one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Picked {
    /// The audio track, by its index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<usize>,
    /// The subtitles shown, or none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitles: Option<Subtitles>,
}

/// Subtitles picked: a track, by its index, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subtitles {
    Off,
    Track(usize),
}

impl Picked {
    /// Nothing picked.
    pub fn is_empty(&self) -> bool {
        self.audio.is_none() && self.subtitles.is_none()
    }

    /// Note track `index` of `kind` picked, or for subtitles none.
    pub fn pick(&mut self, kind: TrackKind, index: Option<usize>) {
        match kind {
            TrackKind::Audio => self.audio = index.or(self.audio),
            TrackKind::Subtitle => {
                self.subtitles = Some(index.map_or(Subtitles::Off, Subtitles::Track));
            }
            TrackKind::Video => {}
        }
    }
}

/// The tracks to play as a file opens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wanted {
    /// Those picked in it before.
    pub picked: Picked,
    /// The languages to play its audio in, first first, as codes.
    pub audio_languages: Vec<String>,
    /// The languages to show its subtitles in, first first.
    pub subtitle_languages: Vec<String>,
}

/// Whether `track` is in `language`.
fn in_language(track: &Track, language: &str) -> bool {
    track
        .language
        .as_deref()
        .is_some_and(|its| same_language(its, language))
}

impl Wanted {
    /// The audio track to play, if one is wanted: the one picked before, if
    /// the file has it; else in the first language preferred it has its
    /// default track in that language, or else its first. `None` leaves it
    /// to the file.
    pub fn audio_track(&self, tracks: &[Track]) -> Option<usize> {
        let audio = || tracks.iter().filter(|t| t.kind == TrackKind::Audio);
        if let Some(index) = self.picked.audio
            && audio().any(|t| t.index == index)
        {
            return Some(index);
        }
        self.audio_languages.iter().find_map(|language| {
            let spoken = || audio().filter(|t| in_language(t, language));
            spoken()
                .find(|t| t.default)
                .or_else(|| spoken().next())
                .map(|t| t.index)
        })
    }

    /// The subtitles to show, if any: those picked before, if the file has
    /// them, or none if they were hidden. Else in the first language
    /// preferred it has: a file beside the video, else its default track in
    /// that language, else its first, forced ones last. Else a subtitle
    /// file beside the video, which someone put there to be shown, else the
    /// ones the file wants shown: its default track, or else a forced one.
    pub fn subtitle_track(&self, tracks: &[Track]) -> Option<usize> {
        let subtitles = || tracks.iter().filter(|t| t.kind == TrackKind::Subtitle);
        match self.picked.subtitles {
            Some(Subtitles::Off) => return None,
            Some(Subtitles::Track(index)) if subtitles().any(|t| t.index == index) => {
                return Some(index);
            }
            _ => {}
        }
        let preferred = self.subtitle_languages.iter().find_map(|language| {
            let written = || subtitles().filter(|t| in_language(t, language));
            let whole = || written().filter(|t| !t.forced);
            whole()
                .find(|t| t.file.is_some())
                .or_else(|| whole().find(|t| t.default))
                .or_else(|| whole().next())
                .or_else(|| written().next())
        });
        let files = || subtitles().filter(|t| t.file.is_some() && !t.forced);
        preferred
            .or_else(|| files().find(|t| t.default))
            .or_else(|| files().next())
            .or_else(|| subtitles().find(|t| t.default))
            .or_else(|| subtitles().find(|t| t.forced))
            .map(|t| t.index)
    }
}

#[cfg(test)]
#[path = "tests/picking.rs"]
mod tests;
