use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Corner, CursorStyle, DispatchPhase, ElementId, Entity, Hitbox, HitboxBehavior, Hsla,
    MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    SharedString, Window, anchored, canvas, deferred, div, px, relative,
};

use super::{ComponentSize, Meter, Panel, Sizable};
use crate::ui::theme::controls::{
    METER_HEIGHT, SLIDER_HEIGHT, SLIDER_KNOB, SLIDER_KNOB_DRAGGED, SLIDER_MIN_WIDTH, SLIDER_NOTCH,
    SLIDER_TIP_GAP,
};
use crate::ui::theme::{Theme, menu, overlay};

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
type Tip = Box<dyn Fn(f32) -> Option<SharedString> + 'static>;

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
    tip: Option<Tip>,
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
            tip: None,
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

    /// A tip over the point the mouse is at while it's over the slider or
    /// drags it: what `tip` says of that point, from 0 to 1, if anything.
    pub fn tip(mut self, tip: impl Fn(f32) -> Option<SharedString> + 'static) -> Self {
        self.tip = Some(Box::new(tip));
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
        // Where along it the mouse is, from 0 to 1, for a tip.
        let mouse = self
            .tip
            .is_some()
            .then(|| window.use_keyed_state(self.id.clone(), cx, |_, _| None::<f32>));
        let tip = self.tip.zip(mouse.as_ref()).and_then(|(tip, mouse)| {
            let at = (*mouse.read(cx))?;
            tip(at).map(|text| (at, text))
        });

        // Tests find a slider by its id.
        let selector = self.id.clone();
        let tip_selector = format!("{}-tip", self.id);
        div()
            .id(self.id)
            .debug_selector(move || selector.to_string())
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
                    )
                    .when_some(tip, |this, (at, text)| {
                        this.child(render_tip(text, at, tip_selector))
                    }),
            )
            .child(drag_area(inset, dragging, mouse, on_change))
    }
}

/// The tip over the point `at` along the track, from 0 to 1: a note
/// centered over it, over everything else, kept inside the window.
fn render_tip(text: SharedString, at: f32, selector: String) -> impl IntoElement {
    // As wide as nothing, so what's in it is centered over the point.
    div()
        .absolute()
        .top(-SLIDER_TIP_GAP)
        .left(relative(at))
        .w_0()
        .flex()
        .justify_center()
        .child(deferred(
            anchored()
                .anchor(Corner::BottomLeft)
                .snap_to_window_with_margin(menu::WINDOW_MARGIN)
                .child(
                    div()
                        .debug_selector(move || selector)
                        .child(Panel::note(text).with_size(ComponentSize::Small)),
                ),
        ))
}

/// Where the mouse drags the slider: a press on it starts a slide, which
/// follows the mouse wherever it goes until the button is let go.
fn drag_area(
    inset: Pixels,
    dragging: Rc<Cell<bool>>,
    mouse: Option<Entity<Option<f32>>>,
    on_change: ChangeHandler,
) -> impl IntoElement {
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
            if let Some(mouse) = mouse {
                follow_mouse(mouse, hitbox.clone(), dragging.clone(), fraction, window);
            }

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
    .size_full()
}

/// Keep `mouse` where the mouse is along the slider, as `fraction` has
/// it, while it's over the slider or drags it, and nowhere otherwise.
/// Listening before the slide does, it hears the mouse after it, as
/// listeners hear the mouse from the last: by then, a slide let go has
/// ended.
fn follow_mouse(
    mouse: Entity<Option<f32>>,
    hitbox: Hitbox,
    dragging: Rc<Cell<bool>>,
    fraction: impl Fn(Point<Pixels>) -> f32 + 'static,
    window: &mut Window,
) {
    let follow = Rc::new(
        move |position: Option<Point<Pixels>>, window: &Window, cx: &mut App| {
            let at = position
                .filter(|_| hitbox.is_hovered(window) || dragging.get())
                .map(&fraction);
            mouse.update(cx, |mouse, cx| {
                if *mouse != at {
                    *mouse = at;
                    cx.notify();
                }
            });
        },
    );
    window.on_mouse_event({
        let follow = follow.clone();
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                follow(Some(event.position), window, cx);
            }
        }
    });
    window.on_mouse_event({
        let follow = follow.clone();
        move |event: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                follow(Some(event.position), window, cx);
            }
        }
    });
    window.on_mouse_event(move |_: &MouseExitEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            follow(None, window, cx);
        }
    });
}

#[cfg(test)]
#[path = "tests/slider.rs"]
mod tests;
