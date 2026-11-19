//! A file's parts, as its chapters have them: which one a time is in, and
//! where going to the next or the one before lands.

/// How near its start a time counts as in a chapter, for a seek that lands
/// a hair before it.
const AT_START: f64 = 0.01;

/// A part of a file, on the playback timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    pub title: Option<String>,
}

impl Chapter {
    /// Its title, or its place among them ("Chapter 2") without one.
    pub fn name(&self, index: usize) -> String {
        self.title
            .clone()
            .unwrap_or_else(|| format!("Chapter {}", index + 1))
    }
}

/// The chapter `time` is in: the last to start by then.
pub fn at(chapters: &[Chapter], time: f64) -> Option<usize> {
    chapters
        .iter()
        .rposition(|chapter| chapter.start <= time + AT_START)
}

/// The chapter after the one `time` is in, or the first, before it starts.
pub fn next(chapters: &[Chapter], time: f64) -> Option<usize> {
    let index = at(chapters, time).map_or(0, |index| index + 1);
    (index < chapters.len()).then_some(index)
}

/// Back to the start of the chapter `time` is in, once it's more than
/// `restart` seconds in; nearer its start, to the one before.
pub fn previous(chapters: &[Chapter], time: f64, restart: f64) -> Option<usize> {
    let index = at(chapters, time)?;
    if time - chapters[index].start > restart {
        Some(index)
    } else {
        Some(index.saturating_sub(1))
    }
}

/// Where the chapters after the first start, from 0 to 1 along `duration`,
/// to mark on the seek bar.
pub fn marks(chapters: &[Chapter], duration: f64) -> Vec<f32> {
    if duration <= 0.0 {
        return Vec::new();
    }
    chapters
        .iter()
        .map(|chapter| chapter.start / duration)
        .filter(|&fraction| fraction > 0.0 && fraction < 1.0)
        .map(|fraction| fraction as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapters() -> Vec<Chapter> {
        [
            (0.0, 10.0, "Opening"),
            (10.0, 30.0, "Middle"),
            (30.0, 40.0, ""),
        ]
        .into_iter()
        .map(|(start, end, title)| Chapter {
            start,
            end,
            title: (!title.is_empty()).then(|| title.to_owned()),
        })
        .collect()
    }

    #[test]
    fn a_time_is_in_the_last_chapter_to_start_by_then() {
        let chapters = chapters();
        assert_eq!(at(&chapters, 0.0), Some(0));
        assert_eq!(at(&chapters, 9.9), Some(0));
        assert_eq!(at(&chapters, 9.995), Some(1), "a seek landing a hair early");
        assert_eq!(at(&chapters, 35.0), Some(2));
        let late = &chapters[1..];
        assert_eq!(at(late, 5.0), None, "before the first starts");
        assert_eq!(at(&[], 5.0), None);
    }

    #[test]
    fn next_goes_to_the_chapter_after_and_none_after_the_last() {
        let chapters = chapters();
        assert_eq!(next(&chapters, 0.0), Some(1));
        assert_eq!(next(&chapters, 12.0), Some(2));
        assert_eq!(next(&chapters, 31.0), None);
        assert_eq!(next(&chapters[1..], 5.0), Some(0), "the first, before it");
        assert_eq!(next(&[], 5.0), None);
    }

    #[test]
    fn previous_restarts_the_chapter_unless_near_its_start() {
        let chapters = chapters();
        assert_eq!(previous(&chapters, 25.0, 2.0), Some(1), "its start");
        assert_eq!(previous(&chapters, 11.0, 2.0), Some(0), "the one before");
        assert_eq!(previous(&chapters, 1.0, 2.0), Some(0), "the first's start");
        assert_eq!(previous(&chapters[1..], 5.0, 2.0), None);
    }

    #[test]
    fn chapters_are_named_by_their_titles_or_places() {
        let chapters = chapters();
        assert_eq!(chapters[0].name(0), "Opening");
        assert_eq!(chapters[2].name(2), "Chapter 3");
    }

    #[test]
    fn the_chapters_after_the_first_are_marked_along_the_bar() {
        assert_eq!(marks(&chapters(), 40.0), [0.25, 0.75]);
        assert!(marks(&chapters(), 0.0).is_empty());
    }
}
