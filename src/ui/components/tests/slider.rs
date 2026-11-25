//! A slider's tip, on a slider across a window of its own.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Context, Modifiers, MouseButton, MouseExitEvent, Pixels, Point, TestAppContext,
    VisualTestContext, Window, div, point, px,
};

use super::Slider;
use crate::app;
use crate::ui::testing::{find, painted, shows};
use crate::ui::theme::menu;

/// A slider across the window, whose tip says where along it the mouse
/// is, but nothing over its last tenth.
struct Bar(Rc<Cell<bool>>);

impl Render for Bar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().items_center().child(
            Slider::new("bar", 0.5, self.0.clone())
                .tip(|at| (at < 0.9).then(|| format!("{:.0} of 100", at * 100.).into())),
        )
    }
}

/// The app as it starts, with a window of just the slider, in front.
fn open(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(app::set_up_for_tests);
    let (_, cx) = cx.add_window_view(|_, _| Bar(Rc::default()));
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx
}

fn move_to(cx: &mut VisualTestContext, to: Point<Pixels>, button: Option<MouseButton>) {
    cx.simulate_mouse_move(to, button, Modifiers::none());
    cx.run_until_parked();
}

/// The lines of text drawn in the tip.
fn tip_lines(cx: &mut VisualTestContext) -> Vec<String> {
    let tip = find(cx, "bar-tip");
    painted(cx)
        .into_iter()
        .filter(|(_, at)| tip.contains(&at.center()))
        .map(|(text, _)| text.to_string())
        .collect()
}

#[gpui::test]
fn its_tip_says_in_a_line_over_the_mouse_what_its_given_for_that_point(cx: &mut TestAppContext) {
    let cx = open(cx);
    let bar = find(cx, "bar");

    move_to(cx, bar.center(), None);
    assert_eq!(tip_lines(cx), ["50 of 100"]);
    let tip = find(cx, "bar-tip");
    assert!(
        (tip.center().x - bar.center().x).abs() <= px(1.),
        "{tip:?} is centered over the mouse"
    );
    assert!(tip.bottom() < bar.top(), "{tip:?} is over the slider");

    // At the start, by the window's edge, it's kept inside the window;
    // at the end, it says nothing.
    move_to(cx, point(bar.left(), bar.center().y), None);
    assert_eq!(tip_lines(cx), ["0 of 100"]);
    let tip = find(cx, "bar-tip");
    assert!(tip.left() >= menu::WINDOW_MARGIN, "{tip:?}");
    move_to(cx, point(bar.right() - px(1.), bar.center().y), None);
    assert!(!shows(cx, "bar-tip"));
}

#[gpui::test]
fn its_tip_shows_while_the_mouse_is_over_it_or_drags_it(cx: &mut TestAppContext) {
    let cx = open(cx);
    let bar = find(cx, "bar");
    let above = |x: Pixels| point(x, bar.top() - bar.size.height * 3.);

    move_to(cx, above(bar.center().x), None);
    assert!(!shows(cx, "bar-tip"), "off it");
    move_to(cx, bar.center(), None);
    assert!(
        cx.debug_bounds("bar-tip").is_some(),
        "drawn as the mouse moves, though nothing else draws the window again"
    );
    cx.simulate_event(MouseExitEvent {
        position: bar.center(),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    cx.run_until_parked();
    assert!(!shows(cx, "bar-tip"), "out of the window");

    // Dragged, it follows the knob off the slider too, until let go.
    move_to(cx, bar.center(), None);
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::none());
    let x = bar.left() + bar.size.width * 0.25;
    move_to(cx, above(x), Some(MouseButton::Left));
    let tip = find(cx, "bar-tip");
    assert!((tip.center().x - x).abs() <= px(1.), "{tip:?}");
    cx.simulate_mouse_up(above(x), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert!(!shows(cx, "bar-tip"), "let go off it");
}
