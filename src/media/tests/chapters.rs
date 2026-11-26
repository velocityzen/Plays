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

/// A chapter of `title`, from `start` to `end`.
fn titled(start: f64, end: f64, title: &str) -> Chapter {
    Chapter {
        start,
        end,
        title: Some(title.to_owned()),
    }
}

#[test]
fn a_chapter_is_an_intro_or_credits_as_its_title_says() {
    let part = |title: &str| titled(0.0, 1.0, title).part();
    for title in [
        "Intro",
        "OPENING",
        "OP",
        "OP2",
        "Opening 2",
        "Opening (2)",
        "Opening Credits",
        "Title Sequence",
    ] {
        assert_eq!(part(title), Some(Part::Intro), "{title}");
    }
    for title in [
        "Credits",
        "End Credits",
        "ending",
        "ED",
        "ED1",
        "End-Credits",
        "Outro",
        "Closing Credits",
    ] {
        assert_eq!(part(title), Some(Part::Credits), "{title}");
    }
    // Story, however it starts or ends.
    for title in [
        "Opening Scene",
        "Introduction",
        "The End",
        "End",
        "Middle",
        "Chapter 1",
        "2",
        "",
    ] {
        assert_eq!(part(title), None, "{title}");
    }
    let untitled = Chapter {
        title: None,
        ..titled(0.0, 1.0, "")
    };
    assert_eq!(untitled.part(), None);
}

#[test]
fn skipping_goes_past_the_intro_or_credits_playing() {
    let chapters = [
        titled(0.0, 5.0, "Recap"),
        titled(5.0, 10.0, "Opening"),
        titled(10.0, 12.0, "Opening 2"),
        titled(12.0, 30.0, "Part A"),
        titled(30.0, 40.0, "Ending"),
        titled(40.0, 45.0, "Preview"),
    ];
    assert_eq!(skip(&chapters, 2.0), None, "a recap is story");
    // An opening in two parts is skipped whole, from either.
    assert_eq!(skip(&chapters, 5.0), Some((Part::Intro, 12.0)));
    assert_eq!(skip(&chapters, 11.0), Some((Part::Intro, 12.0)));
    assert_eq!(
        skip(&chapters, 9.995),
        Some((Part::Intro, 12.0)),
        "landed a hair early"
    );
    assert_eq!(skip(&chapters, 20.0), None);
    assert_eq!(skip(&chapters, 35.0), Some((Part::Credits, 40.0)));
    assert_eq!(skip(&chapters, 42.0), None);
    assert_eq!(skip(&[], 1.0), None);
    // An intro run straight into credits is skipped to its own end.
    let short = [titled(0.0, 1.0, "Intro"), titled(1.0, 2.0, "Credits")];
    assert_eq!(skip(&short, 0.5), Some((Part::Intro, 1.0)));
}
