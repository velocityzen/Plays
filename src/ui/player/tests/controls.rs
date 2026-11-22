use gpui::px;

use super::{Chapter, clearance, format_time, height, time_label};

#[test]
fn subtitles_keep_clear_of_the_controls_only_while_they_show() {
    for live in [false, true] {
        assert_eq!(clearance(true, live), height(live));
        assert!(clearance(true, live) > px(0.));
        assert_eq!(clearance(false, live), px(0.));
    }
}

#[test]
fn the_time_says_which_chapter_it_is_in() {
    let chapter = |start: f64, title: &str| Chapter {
        start,
        end: start + 60.0,
        title: Some(title.to_owned()),
    };
    let chapters = [chapter(0.0, "Opening"), chapter(60.0, "Middle")];
    assert_eq!(
        time_label(75.0, Some(120.0), &chapters),
        "1:15 / 2:00 · Middle"
    );
    assert_eq!(
        time_label(75.0, Some(120.0), &chapters[..1]),
        "1:15 / 2:00",
        "not for a file of one part"
    );
    assert_eq!(time_label(5.0, None, &[]), "0:05");
}

#[test]
fn formats_times() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(59.9), "0:59");
    assert_eq!(format_time(61.0), "1:01");
    assert_eq!(format_time(3725.0), "1:02:05");
    assert_eq!(format_time(-3.0), "0:00");
}
