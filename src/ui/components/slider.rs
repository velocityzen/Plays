use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, CursorStyle, DispatchPhase, ElementId, HitboxBehavior, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, canvas, div, px, relative,
};

use super::{ComponentSize, Meter, Sizable};
use crate::ui::theme::controls::{
    METER_HEIGHT, SLIDER_HEIGHT, SLIDER_KNOB, SLIDER_KNOB_DRAGGED, SLIDER_MIN_WIDTH, SLIDER_NOTCH,
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
/// even outside its own bounds. Its knob stays within the track, so at
/// either end it lines up with what is above or below the slider.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    dragging: Rc<Cell<bool>>,
    overlay: bool,
    fill: Option<Hsla>,
    /// Where notches are cut across the track, from 0 to 1.
    notches: Vec<f32>,
    size: ComponentSize,
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
            notches: Vec::new(),
            size: ComponentSize::Medium,
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

    /// Notches cut across the track at these points, from 0 to 1, as where
    /// a chapter starts; they line up with the knob there.
    pub fn notches(mut self, notches: Vec<f32>) -> Self {
        self.notches = notches;
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

impl Sizable for Slider {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let (track, fill, knob_color, notch) = if self.overlay {
            (
                overlay::track(),
                overlay::text(),
                overlay::text(),
                overlay::notch(),
            )
        } else {
            (theme.track, theme.accent, theme.accent, theme.background)
        };
        let fill = self.fill.unwrap_or(fill);
        let value = self.value.clamp(0.0, 1.0);
        let dragging = self.dragging;
        let on_change = self.on_change.unwrap_or_else(|| Rc::new(|_, _, _| {}));
        let size = self.size;
        let height = size.pick(SLIDER_HEIGHT);
        let bar = size.pick(METER_HEIGHT);
        // The knob's center travels between half its resting size from
        // either end; dragged, it grows about that center.
        let inset = size.pick(SLIDER_KNOB) / 2.;
        let knob = size.pick(if dragging.get() {
            SLIDER_KNOB_DRAGGED
        } else {
            SLIDER_KNOB
        });

        div()
            .id(self.id)
            .relative()
            .flex()
            .items_center()
            .h(height)
            .w_full()
            .min_w(SLIDER_MIN_WIDTH)
            .child(Meter::new(value).track(track).fill(fill).with_size(size))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(inset)
                    .right(inset)
                    .children(self.notches.iter().enumerate().map(|(index, &at)| {
                        div()
                            .debug_selector(move || format!("notch-{index}"))
                            .absolute()
                            .top((height - bar) / 2.)
                            .left(relative(at.clamp(0.0, 1.0)))
                            .ml(-SLIDER_NOTCH / 2.)
                            .w(SLIDER_NOTCH)
                            .h(bar)
                            .bg(notch)
                    }))
                    .child(
                        div()
                            .absolute()
                            .top((height - knob) / 2.)
                            .left(relative(value))
                            .ml(-knob / 2.)
                            .size(knob)
                            .rounded_full()
                            .bg(knob_color),
                    ),
            )
            .child(
                canvas(
                    |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |bounds, hitbox, window, _| {
                        window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
                        // Where the knob's center would be under the mouse.
                        let fraction = move |position: Point<Pixels>| -> f32 {
                            let travel = bounds.size.width - inset * 2.;
                            if travel <= px(0.) {
                                return 0.0;
                            }
                            ((position.x - bounds.left() - inset) / travel).clamp(0.0, 1.0)
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
