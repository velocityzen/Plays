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
