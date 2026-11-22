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
mod tests {
    use super::*;

    /// A track of `kind` at `index`, in `language`.
    fn track(index: usize, kind: TrackKind, language: Option<&str>) -> Track {
        Track {
            index,
            kind,
            language: language.map(str::to_owned),
            title: None,
            codec: "aac".to_owned(),
            format: None,
            default: false,
            forced: false,
            hearing_impaired: false,
            file: None,
        }
    }

    fn audio(index: usize, language: &str) -> Track {
        track(index, TrackKind::Audio, Some(language))
    }

    fn subtitles(index: usize, language: &str) -> Track {
        track(index, TrackKind::Subtitle, Some(language))
    }

    fn default(mut track: Track) -> Track {
        track.default = true;
        track
    }

    fn forced(mut track: Track) -> Track {
        track.forced = true;
        track
    }

    fn beside(mut track: Track) -> Track {
        track.file = Some("Movie.srt".into());
        track
    }

    fn wanting(audio: &[&str], subtitles: &[&str]) -> Wanted {
        Wanted {
            picked: Picked::default(),
            audio_languages: audio.iter().map(|&l| l.to_owned()).collect(),
            subtitle_languages: subtitles.iter().map(|&l| l.to_owned()).collect(),
        }
    }

    #[test]
    fn audio_plays_in_the_first_language_preferred_the_file_has() {
        let tracks = [
            track(0, TrackKind::Video, None),
            default(audio(1, "jpn")),
            audio(2, "eng"),
            audio(3, "fre"),
            subtitles(4, "eng"),
        ];
        assert_eq!(wanting(&[], &[]).audio_track(&tracks), None, "the file's");
        assert_eq!(wanting(&["eng"], &[]).audio_track(&tracks), Some(2));
        assert_eq!(
            wanting(&["ger", "fre", "eng"], &[]).audio_track(&tracks),
            Some(3)
        );
        assert_eq!(
            wanting(&["en"], &[]).audio_track(&tracks),
            Some(2),
            "any code"
        );
        assert_eq!(wanting(&["ger"], &[]).audio_track(&tracks), None);
        assert_eq!(
            wanting(&["eng"], &["fre"]).audio_track(&tracks),
            Some(2),
            "subtitles' languages are theirs"
        );
    }

    #[test]
    fn audio_in_a_language_is_its_default_track_else_its_first() {
        let tracks = [
            audio(1, "eng"),
            default(audio(2, "eng")),
            audio(3, "fre"),
            audio(4, "fre"),
            track(5, TrackKind::Audio, None),
        ];
        assert_eq!(wanting(&["eng"], &[]).audio_track(&tracks), Some(2));
        assert_eq!(wanting(&["fre"], &[]).audio_track(&tracks), Some(3));
    }

    #[test]
    fn audio_picked_in_the_file_plays_again_if_it_has_it() {
        let tracks = [
            track(0, TrackKind::Video, None),
            audio(1, "eng"),
            audio(2, "fre"),
            subtitles(3, "ger"),
        ];
        let mut wanted = wanting(&["eng"], &[]);
        wanted.picked.audio = Some(2);
        assert_eq!(wanted.audio_track(&tracks), Some(2), "over the language");
        for gone in [0, 3, 9] {
            wanted.picked.audio = Some(gone);
            assert_eq!(wanted.audio_track(&tracks), Some(1), "no audio {gone}");
        }
    }

    #[test]
    fn subtitles_show_in_the_first_language_preferred_the_file_has() {
        let tracks = [
            default(audio(1, "eng")),
            default(subtitles(2, "eng")),
            subtitles(3, "fre"),
            subtitles(4, "ger"),
        ];
        assert_eq!(
            wanting(&[], &[]).subtitle_track(&tracks),
            Some(2),
            "the default"
        );
        assert_eq!(wanting(&[], &["fre"]).subtitle_track(&tracks), Some(3));
        assert_eq!(
            wanting(&[], &["ita", "de", "fr"]).subtitle_track(&tracks),
            Some(4)
        );
        assert_eq!(wanting(&[], &["ita"]).subtitle_track(&tracks), Some(2));
        assert_eq!(
            wanting(&["ger"], &[]).subtitle_track(&tracks),
            Some(2),
            "audio's languages are its own"
        );
    }

    #[test]
    fn subtitles_in_a_language_are_a_file_beside_else_the_default_else_the_first_whole() {
        let tracks = [
            forced(subtitles(1, "eng")),
            subtitles(2, "eng"),
            default(subtitles(3, "eng")),
            beside(subtitles(4, "eng")),
            forced(subtitles(5, "fre")),
            subtitles(6, "ger"),
            subtitles(7, "ger"),
        ];
        let show =
            |language: &str, tracks: &[Track]| wanting(&[], &[language]).subtitle_track(tracks);
        assert_eq!(show("eng", &tracks), Some(4), "the file beside");
        assert_eq!(show("eng", &tracks[..3]), Some(3), "the default");
        assert_eq!(show("eng", &tracks[..2]), Some(2), "the first whole");
        assert_eq!(show("eng", &tracks[..1]), Some(1), "forced, if that's all");
        assert_eq!(show("fre", &tracks), Some(5), "forced, if that's all");
        assert_eq!(show("ger", &tracks), Some(6));
        let mut forced_beside = beside(subtitles(8, "ita"));
        forced_beside.forced = true;
        let tracks = [subtitles(1, "ita"), forced_beside];
        assert_eq!(show("ita", &tracks), Some(1), "whole first");
    }

    #[test]
    fn subtitles_with_no_language_preferred_are_as_the_file_wants() {
        let none = wanting(&[], &[]);
        let show = |tracks: &[Track]| none.subtitle_track(tracks);
        let tracks = [
            subtitles(1, "eng"),
            forced(subtitles(2, "eng")),
            default(subtitles(3, "fre")),
            beside(subtitles(4, "ger")),
            default(beside(subtitles(5, "ita"))),
            forced(beside(subtitles(6, "spa"))),
        ];
        assert_eq!(show(&tracks), Some(5), "the default file beside");
        assert_eq!(show(&tracks[..5]), Some(5));
        assert_eq!(show(&tracks[..4]), Some(4), "a file beside");
        assert_eq!(show(&tracks[..3]), Some(3), "the default");
        assert_eq!(show(&tracks[..2]), Some(2), "a forced one");
        assert_eq!(show(&tracks[..1]), None, "none it wants");
        assert_eq!(show(&[forced(beside(subtitles(6, "spa")))]), Some(6));
        assert_eq!(show(&[]), None);
    }

    #[test]
    fn subtitles_picked_in_the_file_show_again_or_stay_hidden() {
        let tracks = [
            audio(1, "eng"),
            default(subtitles(2, "eng")),
            subtitles(3, "fre"),
        ];
        let mut wanted = wanting(&[], &["eng"]);
        wanted.picked.subtitles = Some(Subtitles::Track(3));
        assert_eq!(wanted.subtitle_track(&tracks), Some(3), "over the language");
        wanted.picked.subtitles = Some(Subtitles::Off);
        assert_eq!(wanted.subtitle_track(&tracks), None, "hidden");
        for gone in [1, 9] {
            wanted.picked.subtitles = Some(Subtitles::Track(gone));
            assert_eq!(
                wanted.subtitle_track(&tracks),
                Some(2),
                "no subtitles {gone}"
            );
        }
    }

    #[test]
    fn picking_notes_audio_and_subtitles_but_not_video() {
        let mut picked = Picked::default();
        assert!(picked.is_empty());
        picked.pick(TrackKind::Video, Some(0));
        assert!(picked.is_empty(), "video isn't kept");
        picked.pick(TrackKind::Audio, Some(2));
        assert_eq!(picked.audio, Some(2));
        assert!(!picked.is_empty());
        picked.pick(TrackKind::Audio, None);
        assert_eq!(picked.audio, Some(2), "audio can't be turned off");
        picked.pick(TrackKind::Subtitle, Some(4));
        assert_eq!(picked.subtitles, Some(Subtitles::Track(4)));
        picked.pick(TrackKind::Subtitle, None);
        assert_eq!(picked.subtitles, Some(Subtitles::Off));
        let subtitles_only = Picked {
            audio: None,
            subtitles: Some(Subtitles::Off),
        };
        assert!(!subtitles_only.is_empty());
    }

    #[test]
    fn picks_are_kept_short() {
        let json = |picked: Picked| serde_json::to_string(&picked).unwrap();
        assert_eq!(json(Picked::default()), "{}");
        let picked = Picked {
            audio: Some(2),
            subtitles: Some(Subtitles::Track(4)),
        };
        assert_eq!(json(picked), r#"{"audio":2,"subtitles":{"track":4}}"#);
        let off = Picked {
            audio: None,
            subtitles: Some(Subtitles::Off),
        };
        assert_eq!(json(off), r#"{"subtitles":"off"}"#);
        for picked in [Picked::default(), picked, off] {
            let read: Picked = serde_json::from_str(&json(picked)).unwrap();
            assert_eq!(read, picked);
        }
    }
}
