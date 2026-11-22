//! Something going wrong as a block is drawn, as it did for a sound file
//! named in Cyrillic dropped on a window, which closed Plays: it draws on,
//! and a block that fails says why.

use gpui::{
    AppContext, Entity, ExternalPaths, FileDropEvent, TestAppContext, TextRun, VisualTestContext,
    WindowHandle, point, px, size,
};
use unicode_segmentation::UnicodeSegmentation;

use super::{drawn_in, open};
use crate::files::test_folder;
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, errors_logged, find, fixture, keep_errors, painted, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

fn blocks(window: WindowHandle<Workspace>, cx: &impl AppContext) -> Vec<Entity<PlayerView>> {
    window
        .read_with(cx, |workspace, _| {
            workspace
                .blocks
                .iter()
                .map(|block| block.view.clone())
                .collect()
        })
        .unwrap()
}

/// Dropped on a window, a sound file named `name` shows its name in its
/// header and over its cover, which its file has none of, whole where it
/// fits and cut short with "…" where it doesn't, as its time is, at each
/// width from narrow to wide: where a cluster of characters starts, as
/// long as fits as drawn, and less than a pixel past its box, which is
/// whole pixels wide. The system's own text system panicked on a Cyrillic
/// name cut short for one width and laid out for another.
fn shows_its_name_cut_short_only_where_it_does_not_fit(
    cx: &mut TestAppContext,
    folder: &str,
    name: &str,
) {
    let folder = test_folder(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    std::fs::copy(fixture("tone.m4a"), &path).unwrap();
    keep_errors();
    let (window, mut cx) = testing::start(cx);
    let position = point(WINDOW_SIZE.width / 2., WINDOW_SIZE.height / 2.);
    let paths = ExternalPaths::for_test([path]);
    cx.simulate_event(FileDropEvent::Entered { position, paths });
    cx.simulate_event(FileDropEvent::Submit { position });
    let opened = wait_until(&mut cx, |cx| {
        blocks(window, cx)
            .first()
            .is_some_and(|block| block.read_with(cx, |view, _| view.player().is_known()))
    });
    assert!(opened, "{name} opens");

    cx.simulate_resize(size(px(900.), px(540.)));
    assert_eq!(drawn_in(&mut cx, "block-title"), [name]);
    let lines = painted(&mut cx);
    let names = lines.iter().filter(|(text, _)| text == name).count();
    assert_eq!(names, 2, "in its header and over its cover: {lines:?}");
    assert!(
        lines.iter().any(|(text, _)| text == "0:00 / 0:01"),
        "{lines:?}"
    );

    cx.simulate_resize(size(px(300.), px(400.)));
    let title = drawn_in(&mut cx, "block-title");
    let [title] = title.as_slice() else {
        panic!("one line of title: {title:?}");
    };
    let shown = title.strip_suffix('…').expect("cut short with an ellipsis");
    assert!(
        name.starts_with(shown) && shown.chars().count() > 3,
        "{title}"
    );
    let lines = painted(&mut cx);
    assert!(
        lines.iter().any(|(text, _)| text == name),
        "whole over its cover: {lines:?}"
    );

    let mut shown_before = 0;
    let mut seen = Vec::new();
    for width in 200..900 {
        cx.simulate_resize(size(px(width as f32), px(400.)));
        let bounds = find(&mut cx, "block-title");
        let lines = painted(&mut cx);
        let title: Vec<_> = lines
            .iter()
            .filter(|(_, at)| bounds.contains(&at.origin))
            .collect();
        let [(title, at)] = title.as_slice() else {
            panic!("one line of title at {width}: {title:?}");
        };
        let shown = match title.strip_suffix('…') {
            Some(shown) => {
                assert!(
                    name.starts_with(shown) && shown.len() < name.len(),
                    "{title} at {width}"
                );
                shown
            }
            None => {
                assert_eq!(title, name, "at {width}");
                name
            }
        };
        assert!(
            at.right() < bounds.right() + px(1.),
            "{title} drawn past its end at {width}: {at:?} in {bounds:?}"
        );
        assert!(
            name.grapheme_indices(true)
                .any(|(start, _)| start == shown.len())
                || shown == name,
            "{title} cut inside a cluster at {width}"
        );
        assert!(shown.len() >= shown_before, "{title} shorter at {width}");
        shown_before = shown.len();
        seen.push((
            width,
            title.clone(),
            shown.len(),
            at.size.width,
            bounds.size.width,
        ));
    }
    assert_eq!(shown_before, name.len(), "whole where there's room");
    // Cut short in its room, the longest that fits there: no longer one,
    // nor the whole name, seen at another width is as narrow as that room.
    for (width, title, shown, _, room) in &seen {
        for (_, longer, _, drawn, _) in seen.iter().filter(|seen| seen.2 > *shown) {
            assert!(
                *drawn > *room - px(0.5),
                "{title} at {width}, though {longer}, {drawn:?} wide, fits its {room:?}"
            );
        }
    }

    assert_eq!(
        errors_logged(),
        Vec::<String>::new(),
        "its text was laid out as given"
    );
}

#[test]
fn a_sound_file_named_in_cyrillic_shows_its_name_cut_short_only_where_it_does_not_fit() {
    shows_its_name_cut_short_only_where_it_does_not_fit(
        &mut TestAppContext::with_native_text(),
        "cyrillic-name",
        "01 Рэп больше не кал.m4a",
    );
}

#[test]
fn a_sound_file_named_in_chinese_shows_its_name_cut_short_only_where_it_does_not_fit() {
    shows_its_name_cut_short_only_where_it_does_not_fit(
        &mut TestAppContext::with_native_text(),
        "chinese-name",
        "01 月亮代表我的心 邓丽君 1977.m4a",
    );
}

#[test]
fn a_sound_file_named_in_arabic_shows_its_name_cut_short_only_where_it_does_not_fit() {
    shows_its_name_cut_short_only_where_it_does_not_fit(
        &mut TestAppContext::with_native_text(),
        "arabic-name",
        "01 الأطلال أم كلثوم.m4a",
    );
}

#[test]
fn a_sound_file_named_with_letters_and_their_marks_apart_shows_its_name_cut_short_only_where_it_does_not_fit()
 {
    shows_its_name_cut_short_only_where_it_does_not_fit(
        &mut TestAppContext::with_native_text(),
        "decomposed-name",
        "01 Мои\u{306} е\u{308}жик в тумане.m4a",
    );
}

/// Each mark a glyph of its own, as the no-op text system has them, and as
/// Linux's may.
#[gpui::test]
fn a_sound_file_named_with_letters_and_their_marks_apart_shows_its_name_cut_short_only_where_it_does_not_fit_a_glyph_each(
    cx: &mut TestAppContext,
) {
    shows_its_name_cut_short_only_where_it_does_not_fit(
        cx,
        "marks-apart-name",
        "01 Мои\u{306} е\u{308}жик в тумане.m4a",
    );
}

#[test]
fn a_sound_file_named_with_emoji_shows_its_name_cut_short_only_where_it_does_not_fit() {
    shows_its_name_cut_short_only_where_it_does_not_fit(
        &mut TestAppContext::with_native_text(),
        "emoji-name",
        "01 🎵 Night Drive 🌙🚗 👨‍👩‍👧.m4a",
    );
}

/// Text whose runs end inside a character, before its end or past it, or
/// have no length, is laid out whole, with an error logged, rather than
/// panicking: as if each run ended at the end of the character it ends
/// in, and the last went on to the end. Text with no runs lays out
/// nothing, as it did.
#[test]
fn text_whose_runs_dont_fit_it_is_laid_out_whole_with_an_error_logged() {
    let mut cx = TestAppContext::with_native_text();
    keep_errors();
    let text = "Рэп больше не кал";
    let cx = cx.add_empty_window();
    // Each glyph run's font and how many glyphs it has, and how wide.
    let shapes: Vec<_> = cx.update(|window, _| {
        let style = window.text_style();
        let run = |len| style.to_run(len);
        // A font of another family, with Cyrillic, for a glyph run apart
        // (and another color, as `shape_line` runs on in the font before
        // while the color stays).
        let mono = |len| TextRun {
            color: gpui::red(),
            font: gpui::font(if cfg!(target_os = "macos") {
                "Menlo"
            } else {
                "DejaVu Sans Mono"
            }),
            ..style.to_run(len)
        };
        let shape = |runs: &[TextRun]| {
            let line = window
                .text_system()
                .shape_line(text.into(), px(14.), runs, None);
            let glyphs: Vec<_> = line
                .runs
                .iter()
                .map(|run| (run.font_id, run.glyphs.len()))
                .collect();
            (glyphs, line.width)
        };
        let len = text.len();
        // "Р" is bytes 0..2 and "э" 2..4.
        [
            (vec![run(3)], vec![run(len)]),
            (vec![run(len + 1)], vec![run(len)]),
            (vec![run(3), mono(len - 3)], vec![run(4), mono(len - 4)]),
            (vec![run(0)], vec![run(len)]),
            (vec![], vec![]),
        ]
        .iter()
        .map(|(given, fitted)| (shape(given), shape(fitted)))
        .collect()
    });
    for (given, fitted) in &shapes {
        assert_eq!(given, fitted);
    }
    let [_, _, (split, _), _, (none, _)] = shapes.as_slice() else {
        unreachable!()
    };
    assert_eq!(split.0.iter().map(|(_, glyphs)| glyphs).sum::<usize>(), 17);
    assert_eq!(split.0[0].1, 2, "Р and э in the first run's font");
    assert!(none.0.is_empty());
    let errors = errors_logged();
    assert_eq!(errors.len(), 4, "{errors:?}");
    assert!(
        errors.iter().all(|error| error.contains(text)),
        "{errors:?}"
    );
}

/// A block whose drawing panics shows why, and that alone, logs it and
/// stops, while the others play on; Plays goes on. Another file in it
/// draws as ever.
#[gpui::test]
fn a_block_that_fails_to_draw_says_why_and_stops_while_the_others_play_on(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], true);
    let [broken, other] = blocks(window, &cx).try_into().unwrap();
    let shows =
        |cx: &mut VisualTestContext, text: &str| painted(cx).iter().any(|(line, _)| line == text);
    assert!(shows(&mut cx, "clip.mp4"), "in its header");
    keep_errors();
    broken.update(&mut cx, |view, _| {
        view.panic_while_drawing = Some("the test broke it");
    });
    assert!(shows(&mut cx, "Can't play this file"));
    assert!(shows(&mut cx, "it couldn't be drawn: the test broke it"));
    assert!(!shows(&mut cx, "clip.mp4"), "nor its header");
    broken.read_with(&cx, |view, _| {
        assert_eq!(
            view.player().error().as_deref(),
            Some("it couldn't be drawn: the test broke it")
        );
        assert!(!view.player().is_playing());
    });
    other.read_with(&cx, |view, _| {
        assert!(view.player().error().is_none() && view.player().is_playing());
    });
    assert_eq!(
        errors_logged(),
        ["clip.mp4: it couldn't be drawn: the test broke it"]
    );

    // It shows why, and only that, from then on, not what made it fail
    // once.
    broken.update(&mut cx, |view, _| view.panic_while_drawing = None);
    assert!(shows(&mut cx, "Can't play this file"));
    assert!(!shows(&mut cx, "clip.mp4"));

    broken.update(&mut cx, |view, cx| {
        view.play_instead(fixture("clip-ip.mkv"), cx)
    });
    let opened = wait_until(&mut cx, |cx| {
        broken.read_with(cx, |view, _| view.player().is_known())
    });
    assert!(opened, "the other file opens");
    assert!(!shows(&mut cx, "Can't play this file"));
    assert!(shows(&mut cx, "clip-ip.mkv"), "in its header");
    assert!(broken.read_with(&cx, |view, _| view.player().error().is_none()));
}
