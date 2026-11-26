//! A file's parts, as its chapters have them: which one a time is in,
//! where going to the next or the one before lands, and which are an intro
//! or credits, to skip.

/// How near its start a time counts as in a chapter, for a seek that lands
/// a hair before it.
const AT_START: f64 = 0.01;

/// A part of a file, on the playback timeline.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
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

/// What a chapter holds that's skipped: the titles a show opens with, or
/// the credits it ends with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Intro,
    Credits,
}

impl Chapter {
    /// What it holds that's skipped, as its title says: "Intro",
    /// "Opening", "OP" or "Opening Credits", say; "Credits", "End
    /// Credits", "Ending", "ED" or "Outro"; in any case, with a number
    /// after it or not.
    pub fn part(&self) -> Option<Part> {
        let title = self.title.as_deref()?.to_lowercase();
        let words: Vec<&str> = title
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        let name = words.join(" ");
        // "OP2", "Opening 2".
        let name = name
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .trim_end();
        match name {
            "intro" | "opening" | "op" | "opening credits" | "opening titles" | "opening theme"
            | "title sequence" => Some(Part::Intro),
            "credits" | "end credits" | "ending credits" | "closing credits" | "end titles"
            | "ending" | "ending theme" | "ed" | "outro" => Some(Part::Credits),
            _ => None,
        }
    }
}

/// The intro or credits `time` is in, and where they end: with the
/// chapters right after its of the same part, as an opening in two.
pub fn skip(chapters: &[Chapter], time: f64) -> Option<(Part, f64)> {
    let index = at(chapters, time)?;
    let part = chapters[index].part()?;
    let last = chapters[index..]
        .iter()
        .take_while(|chapter| chapter.part() == Some(part))
        .last()?;
    Some((part, last.end))
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
#[path = "tests/chapters.rs"]
mod tests;
