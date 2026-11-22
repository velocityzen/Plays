//! Finding what's typed: each word typed is looked for, its letters in
//! order though not side by side, in a command's title, its group or one
//! of its keywords, or in a choice's name. Letters that start a word, or
//! follow the one found before, count for more, so `tl` finds Toggle
//! Loop before Subtitles Later, and a title counts for more than the
//! rest.

/// A letter found.
const LETTER: i32 = 1;
/// A letter that starts a word.
const WORD_START: i32 = 8;
/// A letter right after the one found before: more than one starting a
/// word, so `vol` finds Volume before Video Overlay Lock.
const NEXT: i32 = 10;
/// Taken off for each letter before the first found, up to `EARLY`.
const LATE: i32 = 1;
const EARLY: i32 = 6;
/// What a word found in a title counts for more than one found elsewhere.
const TITLE: i32 = 10;

/// Where a query's letters were found in a text, and how well.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub score: i32,
    /// The byte offset in the text of each letter found, in order.
    pub at: Vec<usize>,
}

/// Where `query`'s letters are in `text`, in order, ignoring case, placed
/// where they count for most; `None` if they aren't all there. Spaces in
/// `query` are skipped.
pub fn find(query: &str, text: &str) -> Option<Found> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some(Found::default());
    }
    let letters = letters(text);
    if query.len() > letters.len() {
        return None;
    }
    // best[i][j]: the score of `query[..=i]` found with `query[i]` at
    // `letters[j]`, and where `query[i - 1]` was.
    let mut best: Vec<Vec<Option<(i32, usize)>>> = vec![vec![None; letters.len()]; query.len()];
    for (j, letter) in letters.iter().enumerate() {
        if letter.char == query[0] {
            let late = LATE * i32::try_from(j).unwrap_or(i32::MAX).min(EARLY);
            best[0][j] = Some((LETTER + letter.start_bonus() - late, 0));
        }
    }
    for i in 1..query.len() {
        // The best of `best[i - 1][..j - 1]`, as `j` goes on: a letter
        // found before, but not right before.
        let mut before: Option<(i32, usize)> = None;
        for j in 1..letters.len() {
            if j >= 2
                && let Some((score, _)) = best[i - 1][j - 2]
                && before.is_none_or(|(most, _)| score > most)
            {
                before = Some((score, j - 2));
            }
            if letters[j].char != query[i] {
                continue;
            }
            let next = best[i - 1][j - 1].map(|(score, _)| (score + LETTER + NEXT, j - 1));
            let apart = before.map(|(score, k)| (score + LETTER + letters[j].start_bonus(), k));
            best[i][j] = match (next, apart) {
                (Some(next), Some(apart)) if apart.0 > next.0 => Some(apart),
                (Some(next), _) => Some(next),
                (None, apart) => apart,
            };
        }
    }
    let last = query.len() - 1;
    let (mut j, (score, _)) = best[last]
        .iter()
        .enumerate()
        .filter_map(|(j, found)| found.map(|found| (j, found)))
        .max_by_key(|&(j, (score, _))| (score, std::cmp::Reverse(j)))?;
    let mut at = vec![0; query.len()];
    for i in (0..query.len()).rev() {
        at[i] = letters[j].offset;
        if let Some((_, k)) = best[i][j] {
            j = k;
        }
    }
    Some(Found { score, at })
}

/// A letter of a text, lowercased, where it is, and whether it starts a
/// word.
struct Letter {
    char: char,
    offset: usize,
    starts_word: bool,
}

impl Letter {
    fn start_bonus(&self) -> i32 {
        if self.starts_word { WORD_START } else { 0 }
    }
}

fn letters(text: &str) -> Vec<Letter> {
    let mut letters = Vec::with_capacity(text.len());
    let mut previous: Option<char> = None;
    for (offset, char) in text.char_indices() {
        let starts_word = match previous {
            None => true,
            Some(previous) => {
                !previous.is_alphanumeric()
                    || previous.is_lowercase() && char.is_uppercase()
                    || previous.is_alphabetic() != char.is_alphabetic()
            }
        };
        previous = Some(char);
        // A letter that lowercases to more than one keeps the first, so
        // offsets stay one for each.
        let char = char.to_lowercase().next().unwrap_or(char);
        letters.push(Letter {
            char,
            offset,
            starts_word,
        });
    }
    letters
}

/// What a command is found by.
pub struct Haystack<'a> {
    pub title: &'a str,
    /// What its namespace is called: Playback, Audio.
    pub group: &'a str,
    pub keywords: &'a [&'a str],
}

/// How well `query` finds a command, and the letters of its title found:
/// each word of `query` in its title, its group or a keyword, wherever it
/// counts for most. `None` if a word isn't in any.
pub fn rank(query: &str, haystack: &Haystack) -> Option<Found> {
    let mut found = Found::default();
    for word in query.split_whitespace() {
        let in_title = find(word, haystack.title).map(|mut title| {
            title.score += TITLE;
            title
        });
        let elsewhere = std::iter::once(haystack.group)
            .chain(haystack.keywords.iter().copied())
            .filter_map(|text| find(word, text))
            .map(|found| found.score)
            .max();
        let title = match (in_title, elsewhere) {
            (Some(title), None) => title,
            (Some(title), Some(score)) if title.score >= score => title,
            (_, Some(score)) => {
                found.score += score;
                continue;
            }
            (None, None) => return None,
        };
        found.score += title.score;
        found.at.extend(title.at);
    }
    found.at.sort_unstable();
    found.at.dedup();
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(query: &str, text: &str) -> Option<Vec<usize>> {
        find(query, text).map(|found| found.at)
    }

    fn score(query: &str, text: &str) -> i32 {
        find(query, text).map_or(i32::MIN, |found| found.score)
    }

    #[test]
    fn finds_the_letters_in_order_ignoring_case() {
        assert_eq!(at("loop", "Loop"), Some(vec![0, 1, 2, 3]));
        assert_eq!(at("LP", "Loop"), Some(vec![0, 3]));
        assert_eq!(at("pl", "Loop"), None, "out of order");
        assert_eq!(at("loops", "Loop"), None, "a letter too many");
        assert_eq!(at("", "Loop"), Some(vec![]), "nothing typed finds all");
        assert_eq!(at("x", ""), None);
    }

    #[test]
    fn places_letters_where_words_start() {
        assert_eq!(
            at("tl", "Toggle Loop"),
            Some(vec![0, 7]),
            "the L of Loop, not of Toggle"
        );
        assert_eq!(at("nf", "Next Frame"), Some(vec![0, 5]));
        assert_eq!(at("st", "Subtitles Later"), Some(vec![0, 3]));
    }

    #[test]
    fn letters_side_by_side_count_for_more() {
        assert!(score("vol", "Volume Up") > score("vol", "Video Overlay Lock"));
        assert!(score("loop", "Loop") > score("loop", "Look Out Of Place"));
    }

    #[test]
    fn letters_starting_words_count_for_more_than_letters_inside_them() {
        assert!(score("tl", "Toggle Loop") > score("tl", "Subtitles Later"));
        assert!(score("mu", "Mute/Unmute") > score("mu", "Maximum"));
    }

    #[test]
    fn found_early_counts_for_more() {
        assert!(score("p", "Play/Pause") > score("p", "Restart Playback"));
    }

    #[test]
    fn a_word_starts_after_a_lowercase_letter_before_a_capital_and_at_a_number() {
        assert_eq!(
            at("tc", "tickConsole"),
            Some(vec![0, 4]),
            "the C of Console, not of tick"
        );
        assert_eq!(at("1", "Pick track1"), Some(vec![10]));
    }

    #[test]
    fn letters_lowercasing_to_more_than_one_keep_their_place() {
        assert_eq!(at("is", "İs"), Some(vec![0, 2]));
    }

    fn haystack<'a>(title: &'a str, group: &'a str, keywords: &'a [&'a str]) -> Haystack<'a> {
        Haystack {
            title,
            group,
            keywords,
        }
    }

    #[test]
    fn each_word_is_found_in_the_title_the_group_or_a_keyword() {
        let mute = haystack("Mute/Unmute", "Audio", &["sound", "silence"]);
        assert_eq!(rank("mute", &mute).map(|found| found.at.len()), Some(4));
        assert!(rank("audio mute", &mute).is_some(), "the group");
        assert!(rank("mute audio", &mute).is_some(), "in any order");
        assert!(rank("silence", &mute).is_some(), "a keyword");
        assert!(rank("mute video", &mute).is_none(), "every word");
        assert_eq!(
            rank("audio", &mute).map(|found| found.at),
            Some(vec![]),
            "only the title's letters are noted"
        );
        assert_eq!(rank("", &mute), Some(Found::default()));
    }

    #[test]
    fn a_word_in_the_title_counts_for_more_than_one_elsewhere() {
        let in_title = haystack("Audio Later", "Audio", &[]);
        let in_group = haystack("Next Track", "Audio", &[]);
        assert!(rank("audio", &in_title).unwrap().score > rank("audio", &in_group).unwrap().score);
        let keyword = haystack("Mute/Unmute", "Audio", &["sound"]);
        assert!(
            rank("sound", &keyword).unwrap().at.is_empty(),
            "found where it counts for most"
        );
    }
}
