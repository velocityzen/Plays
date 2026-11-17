use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, CursorStyle, DispatchPhase, ElementId, HitboxBehavior, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, canvas, div, px, relative,
};

use crate::ui::theme::{Theme, overlay};

/// Where a drag is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slide {
    Start,
    Move,
    End,
}

/// What a slider reports while it is dragged.
#[derive(Clone, Copy, Debug)]
pub struct SliderEvent {
    /// From 0 to 1.
    pub value: f32,
    pub slide: Slide,
}

type ChangeHandler = Rc<dyn Fn(&SliderEvent, &mut Window, &mut App) + 'static>;

/// A horizontal 0..1 slider that keeps tracking the mouse while dragging,
/// even outside its own bounds.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    dragging: Rc<Cell<bool>>,
    overlay: bool,
    fill: Option<Hsla>,
    on_change: Option<ChangeHandler>,
}

impl Slider {
    /// `dragging` is set while the slider is dragged; it must live as long as
    /// the view, which can also read it.
    pub fn new(id: impl Into<ElementId>, value: f32, dragging: Rc<Cell<bool>>) -> Self {
        Self {
            id: id.into(),
            value,
            dragging,
            overlay: false,
            fill: None,
            on_change: None,
        }
    }

    /// Colors for drawing over video, which is dark in both themes.
    pub fn overlay(mut self) -> Self {
        self.overlay = true;
        self
    }

    /// The color of the part of the track up to the knob.
    pub fn fill(mut self, color: Hsla) -> Self {
        self.fill = Some(color);
        self
    }

    /// Called when a drag starts, moves and ends.
    pub fn on_change(
        mut self,
        handler: impl Fn(&SliderEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let (track, fill, knob_color) = if self.overlay {
            (overlay::track(), overlay::text(), overlay::text())
        } else {
            (theme.track, theme.accent, theme.accent)
        };
        let fill = self.fill.unwrap_or(fill);
        let value = self.value.clamp(0.0, 1.0);
        let dragging = self.dragging;
        let on_change = self.on_change.unwrap_or_else(|| Rc::new(|_, _, _| {}));
        let knob = if dragging.get() { px(12.) } else { px(10.) };

        div()
            .id(self.id)
            .relative()
            .flex()
            .items_center()
            .h(px(16.))
            .w_full()
            .min_w(px(32.))
            .child(
                div()
                    .w_full()
                    .h(px(4.))
                    .rounded_full()
                    .bg(track)
                    .child(div().h_full().w(relative(value)).rounded_full().bg(fill)),
            )
            .child(
                div()
                    .absolute()
                    .top((px(16.) - knob) / 2.)
                    .left(relative(value))
                    .ml(-knob / 2.)
                    .size(knob)
                    .rounded_full()
                    .bg(knob_color),
            )
            .child(
                canvas(
                    |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |bounds, hitbox, window, _| {
                        window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
                        let fraction = move |position: Point<Pixels>| -> f32 {
                            if bounds.size.width <= px(0.) {
                                return 0.0;
                            }
                            ((position.x - bounds.left()) / bounds.size.width).clamp(0.0, 1.0)
                        };

                        window.on_mouse_event({
                            let (dragging, on_change) = (dragging.clone(), on_change.clone());
                            move |event: &MouseDownEvent, phase, window, cx| {
                                if phase != DispatchPhase::Bubble
                                    || event.button != MouseButton::Left
                                    || !hitbox.is_hovered(window)
                                {
                                    return;
                                }
                                dragging.set(true);
                                cx.stop_propagation();
                                let value = fraction(event.position);
                                on_change(
                                    &SliderEvent {
                                        value,
                                        slide: Slide::Start,
                                    },
                                    window,
                                    cx,
                                );
                            }
                        });

                        window.on_mouse_event({
                            let (dragging, on_change) = (dragging.clone(), on_change.clone());
                            move |event: &MouseMoveEvent, phase, window, cx| {
                                if phase != DispatchPhase::Bubble || !dragging.get() {
                                    return;
                                }
                                // The button was released somewhere we didn't see.
                                let released = event.pressed_button != Some(MouseButton::Left);
                                if released {
                                    dragging.set(false);
                                }
                                let value = fraction(event.position);
                                let slide = if released { Slide::End } else { Slide::Move };
                                on_change(&SliderEvent { value, slide }, window, cx);
                            }
                        });

                        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                            if phase != DispatchPhase::Bubble || !dragging.get() {
                                return;
                            }
                            dragging.set(false);
                            let value = fraction(event.position);
                            on_change(
                                &SliderEvent {
                                    value,
                                    slide: Slide::End,
                                },
                                window,
                                cx,
                            );
                        });
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}
