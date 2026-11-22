use gpui::{Bounds, point, px, size};

use super::beside;
use crate::ui::theme::layout::WINDOW_GAP;

#[test]
fn a_window_opens_beside_the_one_it_came_from() {
    let room = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
    let new = size(px(600.), px(400.));
    let at =
        |x: f32, y: f32, width: f32| Bounds::new(point(px(x), px(y)), size(px(width), px(500.)));

    let right = beside(at(100., 100., 800.), new, room);
    assert_eq!(
        right.origin,
        point(px(900.) + WINDOW_GAP, px(100.)),
        "to the right"
    );
    assert_eq!(right.size, new);

    let left = beside(at(1000., 100., 800.), new, room);
    assert_eq!(
        left.origin.x,
        px(400.) - WINDOW_GAP,
        "to the left, with no room right"
    );

    let middle = beside(at(300., 100., 1400.), new, room);
    assert_eq!(
        middle.origin.x,
        px(660.),
        "centered, with room on neither side"
    );

    let low = beside(at(100., 900., 800.), new, room);
    assert_eq!(
        low.origin.y,
        px(680.),
        "up from the bottom, to stay on the display"
    );

    let wide = beside(at(100., 100., 800.), size(px(2400.), px(400.)), room);
    assert_eq!(
        wide.origin.x,
        px(0.),
        "its left edge on it, when it's wider than that"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn appkit_is_asked_the_screens_names_on_the_main_thread_only() {
    // Tests run off it, as this one does.
    assert!(super::mac::names().is_empty());
    assert_eq!(super::mac::with_mouse(), None);
}
