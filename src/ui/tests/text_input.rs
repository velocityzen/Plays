use gpui::{Entity, TestAppContext, VisualTestContext};

use super::*;
use crate::settings::Settings;
use crate::ui::theme::UiFont;

/// Characters of one to four bytes, and of one or two UTF-16 units:
/// a, é, 中 and 😀.
const WIDE: &str = "aé中😀b";

/// Do something to the input, as its window would.
fn edit<R>(
    cx: &mut VisualTestContext,
    input: &Entity<TextInput>,
    what: impl FnOnce(&mut TextInput, &mut Window, &mut Context<TextInput>) -> R,
) -> R {
    cx.update(|window, cx| input.update(cx, |input, cx| what(input, window, cx)))
}

fn new_input(cx: &mut TestAppContext) -> (Entity<TextInput>, &mut VisualTestContext) {
    let cx = cx.add_empty_window();
    let input = cx.update(|_, cx| cx.new(|cx| TextInput::new("", cx)));
    (input, cx)
}

/// What IME's `setMarkedText:selectedRange:` does.
fn compose(
    cx: &mut VisualTestContext,
    input: &Entity<TextInput>,
    text: &str,
    selected: Range<usize>,
) {
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, text, Some(selected), window, cx);
        });
    });
}

fn type_text(cx: &mut VisualTestContext, input: &Entity<TextInput>, text: &str) {
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, text, window, cx);
        });
    });
}

fn state(cx: &mut VisualTestContext, input: &Entity<TextInput>) -> (String, Range<usize>) {
    cx.update(|_, cx| {
        let input = input.read(cx);
        (input.content.to_string(), input.selected_range.clone())
    })
}

#[gpui::test]
fn composing_keeps_the_cursor_in_what_is_composed(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    compose(cx, &input, "n", 1..1);
    assert_eq!(state(cx, &input), ("n".to_owned(), 1..1));
    compose(cx, &input, "ni", 2..2);
    assert_eq!(state(cx, &input), ("ni".to_owned(), 2..2));
    compose(cx, &input, "你", 1..1);
    assert_eq!(state(cx, &input), ("你".to_owned(), 3..3));
    type_text(cx, &input, "你");
    assert_eq!(state(cx, &input), ("你".to_owned(), 3..3));
}

#[gpui::test]
fn deleting_what_is_composed_leaves_the_text_to_type_into(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    compose(cx, &input, "n", 1..1);
    compose(cx, &input, "ni", 2..2);
    compose(cx, &input, "", 0..0);
    assert_eq!(state(cx, &input), (String::new(), 0..0));
    type_text(cx, &input, "x");
    assert_eq!(state(cx, &input), ("x".to_owned(), 1..1));
}

#[gpui::test]
fn the_selection_counts_within_what_is_composed_after_other_text(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    type_text(cx, &input, "abc");
    compose(cx, &input, "中", 1..1);
    assert_eq!(state(cx, &input), ("abc中".to_owned(), 6..6));
    compose(cx, &input, "中文", 1..1);
    assert_eq!(state(cx, &input), ("abc中文".to_owned(), 6..6));
    type_text(cx, &input, "中文");
    assert_eq!(state(cx, &input), ("abc中文".to_owned(), 9..9));
}

#[gpui::test]
fn ranges_out_of_the_text_are_kept_in_it(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    type_text(cx, &input, "aé");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            let mut actual = None;
            // IME may ask for a range backwards.
            let backwards = Range { start: 2, end: 0 };
            let text = input.text_for_range(backwards, &mut actual, window, cx);
            assert_eq!(text.as_deref(), Some("aé"));
            assert_eq!(actual, Some(0..2));
            input.selected_range = 2..9;
            input.replace_text_in_range(None, "!", window, cx);
        });
    });
    assert_eq!(state(cx, &input), ("a!".to_owned(), 2..2));
}

#[test]
fn offsets_go_between_utf16_and_bytes_through_wide_characters() {
    // Where each character starts, in bytes and in UTF-16, and the end.
    let bytes = [0, 1, 3, 6, 10, 11];
    let units = [0, 1, 2, 3, 5, 6];
    for (byte, unit) in bytes.into_iter().zip(units) {
        assert_eq!(utf16_offset(WIDE, byte), unit, "byte {byte}");
        assert_eq!(utf8_offset(WIDE, unit), byte, "unit {unit}");
    }
    // Inside a character, on to its end.
    assert_eq!(utf16_offset(WIDE, 2), 2, "inside é");
    assert_eq!(utf16_offset(WIDE, 7), 5, "inside 😀");
    assert_eq!(utf8_offset(WIDE, 4), 10, "between 😀's surrogates");
    // Past the end, the end.
    assert_eq!(utf16_offset(WIDE, 99), 6);
    assert_eq!(utf8_offset(WIDE, 99), 11);
    assert_eq!(utf16_offset("", 0), 0);
    assert_eq!(utf8_offset("", 5), 0);
}

#[test]
fn ranges_are_kept_in_order_on_characters_and_in_the_text() {
    assert_eq!(within(WIDE, 0..11), 0..11);
    assert_eq!(within(WIDE, Range { start: 6, end: 3 }), 3..6, "backwards");
    assert_eq!(
        within(WIDE, 2..8),
        1..6,
        "inside é and 😀, back to their starts"
    );
    assert_eq!(within(WIDE, 4..99), 3..11);
    assert_eq!(within("", Range { start: 3, end: 1 }), 0..0);
    assert_eq!(utf8_range(WIDE, &(3..5)), 6..10, "😀, in UTF-16");
    assert_eq!(utf8_range(WIDE, &Range { start: 5, end: 3 }), 6..10);
    assert_eq!(utf8_range(WIDE, &(4..4)), 10..10);
    assert_eq!(utf8_range("", &(0..2)), 0..0);
}

#[gpui::test]
fn backspace_and_delete_take_whole_graphemes(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    // An accent written as a character of its own, a family of three
    // people joined into one emoji, and a CJK character.
    let text = "ae\u{301}👨‍👩‍👧中";
    type_text(cx, &input, text);
    for left in ["ae\u{301}👨‍👩‍👧", "ae\u{301}", "a", "", ""] {
        edit(cx, &input, |input, window, cx| {
            input.backspace(&Backspace, window, cx)
        });
        assert_eq!(state(cx, &input), (left.to_owned(), left.len()..left.len()));
    }
    type_text(cx, &input, text);
    edit(cx, &input, |input, window, cx| {
        input.home(&Home, window, cx)
    });
    for left in ["e\u{301}👨‍👩‍👧中", "👨‍👩‍👧中", "中", "", ""] {
        edit(cx, &input, |input, window, cx| {
            input.delete(&Delete, window, cx)
        });
        assert_eq!(state(cx, &input), (left.to_owned(), 0..0));
    }
}

#[gpui::test]
fn a_selection_turned_round_keeps_its_anchor(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    // 中 is bytes 2 to 5, 😀 5 to 9; in UTF-16, 2 and 3 to 5.
    type_text(cx, &input, "ab中😀");
    edit(cx, &input, |input, window, cx| {
        input.home(&Home, window, cx);
        input.right(&Right, window, cx);
        input.right(&Right, window, cx);
    });
    let left = |cx: &mut VisualTestContext| {
        edit(cx, &input, |input, window, cx| {
            input.select_left(&SelectLeft, window, cx);
            (input.selected_range.clone(), input.selection_reversed)
        })
    };
    let right = |cx: &mut VisualTestContext| {
        edit(cx, &input, |input, window, cx| {
            input.select_right(&SelectRight, window, cx);
            (input.selected_range.clone(), input.selection_reversed)
        })
    };
    assert_eq!(right(cx), (2..5, false));
    assert_eq!(left(cx), (2..2, false));
    assert_eq!(left(cx), (1..2, true), "past the anchor it turns round");
    let told = edit(cx, &input, |input, window, cx| {
        input.selected_text_range(false, window, cx)
    });
    let told = told.unwrap();
    assert_eq!((told.range, told.reversed), (1..2, true));
    assert_eq!(left(cx), (0..2, true));
    assert_eq!(right(cx), (1..2, true), "the far end moves, not the anchor");
    assert_eq!(right(cx), (2..2, true));
    assert_eq!(right(cx), (2..5, false), "and back again");
    assert_eq!(right(cx), (2..9, false));
    let told = edit(cx, &input, |input, window, cx| {
        input.selected_text_range(false, window, cx)
    });
    assert_eq!(told.unwrap().range, 2..5, "in UTF-16");
    type_text(cx, &input, "x");
    assert_eq!(state(cx, &input), ("abx".to_owned(), 3..3));
}

#[gpui::test]
fn ime_edits_wide_text_by_its_utf16_ranges(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    // In UTF-16, 中 is 0, 文 1, and 😀 2 to 4.
    type_text(cx, &input, "中文😀");
    edit(cx, &input, |input, window, cx| {
        input.replace_and_mark_text_in_range(Some(2..4), "にほん", Some(1..2), window, cx);
    });
    assert_eq!(state(cx, &input), ("中文にほん".to_owned(), 9..12));
    let (marked, read) = edit(cx, &input, |input, window, cx| {
        let mut actual = None;
        let read = input.text_for_range(2..5, &mut actual, window, cx);
        assert_eq!(actual, Some(2..5));
        (input.marked_text_range(window, cx), read)
    });
    assert_eq!(marked, Some(2..5));
    assert_eq!(read.as_deref(), Some("にほん"));
    // Committing replaces what's composed, wherever the cursor is.
    type_text(cx, &input, "日本");
    assert_eq!(state(cx, &input), ("中文日本".to_owned(), 12..12));
    let marked = edit(cx, &input, |input, window, cx| {
        input.marked_text_range(window, cx)
    });
    assert_eq!(marked, None);

    // A range that starts and ends inside surrogate pairs takes the
    // characters it ends in.
    edit(cx, &input, |input, window, cx| {
        input.set_text("😀😀", cx);
        input.replace_text_in_range(Some(1..3), "a", window, cx);
    });
    assert_eq!(state(cx, &input), ("😀a".to_owned(), 5..5));
}

#[gpui::test]
fn ime_into_empty_text_and_with_empty_text(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    edit(cx, &input, |input, window, cx| {
        let mut actual = None;
        assert_eq!(
            input
                .text_for_range(0..4, &mut actual, window, cx)
                .as_deref(),
            Some("")
        );
        assert_eq!(actual, Some(0..0));
        input.replace_and_mark_text_in_range(Some(0..0), "", Some(0..0), window, cx);
        assert_eq!(
            input.marked_text_range(window, cx),
            None,
            "nothing is marked"
        );
        input.replace_and_mark_text_in_range(Some(3..7), "😀", None, window, cx);
        assert_eq!(input.marked_text_range(window, cx), Some(0..2));
    });
    assert_eq!(state(cx, &input), ("😀".to_owned(), 4..4));
    edit(cx, &input, |input, window, cx| {
        input.replace_text_in_range(None, "", window, cx);
        input.replace_text_in_range(Some(5..9), "a", window, cx);
    });
    assert_eq!(state(cx, &input), ("a".to_owned(), 1..1));
}

#[gpui::test]
fn ime_is_told_where_wide_characters_are_drawn(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Settings::default());
        cx.set_global(UiFont("Plays Test".into()));
    });
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::new("Search", cx));
    let draw = |cx: &mut VisualTestContext| {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    };
    let field = Bounds::new(point(px(10.), px(20.)), size(px(400.), px(30.)));
    let at = |cx: &mut VisualTestContext, range: Range<usize>| {
        edit(cx, &input, |input, window, cx| {
            input.bounds_for_range(range, field, window, cx)
        })
        .expect("drawn")
    };
    // Empty, its placeholder drawn: the cursor at the start.
    draw(cx);
    let start = at(cx, 0..0);
    assert_eq!(start.size.width, px(0.));
    assert_eq!((start.top(), start.bottom()), (field.top(), field.bottom()));

    // In UTF-16, a is 0, 中 1, and 😀 2 to 4.
    type_text(cx, &input, "a中😀");
    draw(cx);
    let (a, cjk, emoji) = (at(cx, 0..1), at(cx, 1..2), at(cx, 2..4));
    assert_eq!(a.left(), start.left());
    assert!(a.size.width > px(0.) && cjk.size.width > px(0.) && emoji.size.width > px(0.));
    assert_eq!(a.right(), cjk.left());
    assert_eq!(cjk.right(), emoji.left());
    let all = at(cx, 0..4);
    assert_eq!((all.left(), all.right()), (a.left(), emoji.right()));
    assert_eq!(at(cx, Range { start: 4, end: 2 }), emoji, "backwards");
    // Between 😀's surrogates, and past the end: after it.
    for range in [3..3, 9..12] {
        let after = at(cx, range.clone());
        assert_eq!(
            (after.left(), after.size.width),
            (emoji.right(), px(0.)),
            "{range:?}"
        );
    }
}

/// It says its text changed when typed, deleted, composed or set; not when
/// only the cursor or the selection moved.
#[gpui::test]
fn says_when_its_text_changes_not_its_cursor(cx: &mut TestAppContext) {
    let (input, cx) = new_input(cx);
    let edits = std::rc::Rc::new(std::cell::Cell::new(0));
    let counted = edits.clone();
    let _edited = cx.update(|_, cx| {
        cx.subscribe(&input, move |_, _: &Edited, _| {
            counted.set(counted.get() + 1)
        })
    });
    type_text(cx, &input, "abc");
    assert_eq!(edits.get(), 1);
    edit(cx, &input, |input, window, cx| {
        input.left(&Left, window, cx);
        input.right(&Right, window, cx);
        input.home(&Home, window, cx);
        input.end(&End, window, cx);
        input.select_left(&SelectLeft, window, cx);
        input.select_right(&SelectRight, window, cx);
        input.select_all(&SelectAll, window, cx);
    });
    assert_eq!(edits.get(), 1, "moving and selecting aren't edits");
    edit(cx, &input, |input, window, cx| {
        input.backspace(&Backspace, window, cx)
    });
    assert_eq!(edits.get(), 2);
    compose(cx, &input, "n", 1..1);
    assert_eq!(edits.get(), 3);
    edit(cx, &input, |input, _, cx| input.set_text("set", cx));
    assert_eq!(edits.get(), 4);
}
