//! Small reusable controls drawn with plain gpui elements.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Context, CursorStyle, DispatchPhase, Div, ElementId, HitboxBehavior, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Stateful, Svg,
    WeakEntity, Window, canvas, div, px, relative, svg, white,
};

use super::theme::Theme;

pub fn icon(name: &str) -> Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .flex_none()
}

/// A square, borderless button showing one icon.
pub fn icon_button(
    id: impl Into<ElementId>,
    name: &str,
    color: Hsla,
    hover: Hsla,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .child(icon(name).size(px(16.)).text_color(color))
}

/// A regular text button with an optional leading icon.
pub fn button(
    id: impl Into<ElementId>,
    icon_name: Option<&str>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    let (hover, active) = (theme.surface_hover, theme.surface_active);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(28.))
        .px_2p5()
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .text_sm()
        .text_color(theme.text)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .active(move |style| style.bg(active))
        .when_some(icon_name, |this, name| {
            this.child(icon(name).size(px(14.)).text_color(theme.text_muted))
        })
        .child(label.into())
}

/// An on/off switch. Attach `on_click` to flip it.
pub fn switch(id: impl Into<ElementId>, on: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .when(on, |this| this.justify_end())
        .w(px(36.))
        .h(px(20.))
        .p(px(2.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on { theme.accent } else { theme.track })
        .child(div().size(px(16.)).rounded_full().bg(white()))
}

/// A row of mutually exclusive choices.
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(T, &'static str)],
    selected: T,
    theme: &Theme,
    on_select: impl Fn(T, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let (text, muted, surface) = (theme.text, theme.text_muted, theme.surface);
    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(7.))
        .bg(theme.surface_active)
        .children(options.iter().enumerate().map(|(index, &(value, label))| {
            let on_select = on_select.clone();
            let active = value == selected;
            div()
                .id((id, index))
                .flex()
                .items_center()
                .h(px(24.))
                .px_2p5()
                .rounded(px(5.))
                .text_sm()
                .cursor_pointer()
                .when(active, |this| this.bg(surface).text_color(text).shadow_sm())
                .when(!active, |this| {
                    this.text_color(muted)
                        .hover(move |style| style.text_color(text))
                })
                .on_click(move |_, window, cx| on_select(value, window, cx))
                .child(label)
        }))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slide {
    Start,
    Move,
    End,
}

#[derive(Clone, Copy)]
pub struct SliderColors {
    pub track: Hsla,
    pub fill: Hsla,
    pub knob: Hsla,
}

/// A horizontal 0..1 slider that keeps tracking the mouse while dragging,
/// even outside its own bounds. `dragging` must live as long as the view.
pub fn slider<V: 'static>(
    id: impl Into<ElementId>,
    value: f32,
    dragging: Rc<Cell<bool>>,
    view: WeakEntity<V>,
    colors: SliderColors,
    on_change: impl Fn(&mut V, f32, Slide, &mut Window, &mut Context<V>) + 'static,
) -> Stateful<Div> {
    let value = value.clamp(0.0, 1.0);
    let on_change = Rc::new(on_change);
    let knob = if dragging.get() { px(12.) } else { px(10.) };

    div()
        .id(id)
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
                .bg(colors.track)
                .child(
                    div()
                        .h_full()
                        .w(relative(value))
                        .rounded_full()
                        .bg(colors.fill),
                ),
        )
        .child(
            div()
                .absolute()
                .top((px(16.) - knob) / 2.)
                .left(relative(value))
                .ml(-knob / 2.)
                .size(knob)
                .rounded_full()
                .bg(colors.knob),
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
                        let (dragging, view, on_change) =
                            (dragging.clone(), view.clone(), on_change.clone());
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
                            view.update(cx, |view, cx| {
                                on_change(view, value, Slide::Start, window, cx)
                            })
                            .ok();
                        }
                    });

                    window.on_mouse_event({
                        let (dragging, view, on_change) =
                            (dragging.clone(), view.clone(), on_change.clone());
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
                            view.update(cx, |view, cx| on_change(view, value, slide, window, cx))
                                .ok();
                        }
                    });

                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble || !dragging.get() {
                            return;
                        }
                        dragging.set(false);
                        let value = fraction(event.position);
                        view.update(cx, |view, cx| {
                            on_change(view, value, Slide::End, window, cx)
                        })
                        .ok();
                    });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
}

pub fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_time;

    #[test]
    fn formats_times() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(59.9), "0:59");
        assert_eq!(format_time(61.0), "1:01");
        assert_eq!(format_time(3725.0), "1:02:05");
        assert_eq!(format_time(-3.0), "0:00");
    }
}
