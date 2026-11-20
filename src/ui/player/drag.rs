//! Dragging a tile by its picture: onto another tile in its window, to
//! take that one's place; or, as the only tile there or maximized, the
//! window with it, as its title bar does. Not in full screen, where the
//! window stays put.

use gpui::prelude::*;
use gpui::{
    Context, Div, EntityId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, SharedString, Stateful, Window, div,
};

use super::PlayerView;
use crate::ui::components::Panel;
use crate::ui::theme::layout::{DRAG_START, DRAG_TITLE_MAX_WIDTH, DRAG_TITLE_OFFSET};
use crate::ui::theme::overlay;

/// A tile being dragged onto another's place: which one.
pub struct DraggedTile(pub EntityId);

/// What follows the mouse while a tile is dragged: its title, beside the
/// pointer.
struct DraggedTitle {
    title: SharedString,
    /// Where the picture was pressed: gpui draws a drag from there, as far
    /// from the mouse as the picture's corner was.
    pressed: Point<Pixels>,
}

impl Render for DraggedTitle {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // After the pointer, or before it where the window would cut it off.
        let after = window.mouse_position().x + DRAG_TITLE_OFFSET.x + DRAG_TITLE_MAX_WIDTH
            <= window.viewport_size().width;
        let left = if after {
            self.pressed.x + DRAG_TITLE_OFFSET.x
        } else {
            self.pressed.x - DRAG_TITLE_OFFSET.x - DRAG_TITLE_MAX_WIDTH
        };
        // A drag is laid out with no room: the row gives the title its
        // most, and a long one is cut short there.
        div().child(
            div()
                .absolute()
                .left(left)
                .top(self.pressed.y + DRAG_TITLE_OFFSET.y)
                .w(DRAG_TITLE_MAX_WIDTH)
                .flex()
                .when(!after, |row| row.justify_end())
                .child(
                    Panel::note(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(overlay::text())
                            .child(self.title.clone()),
                    )
                    .text_xs()
                    .min_w_0(),
                ),
        )
    }
}

impl PlayerView {
    /// Let the picture, `surface`, drag the tile onto another's place, or
    /// the window with no other tile showing.
    pub(super) fn drag_by_picture(
        &self,
        surface: Stateful<Div>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        if !self.layout.alone && !self.layout.maximized {
            let title = self.title.clone();
            return surface.on_drag(DraggedTile(cx.entity_id()), move |_, pressed, _, cx| {
                cx.new(|_| DraggedTitle {
                    title: title.clone(),
                    pressed,
                })
            });
        }
        if window.is_fullscreen() {
            return surface;
        }
        surface
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    this.window_press = Some(event.position);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.window_press = None),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, _| {
                let Some(pressed) = this.window_press else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    this.window_press = None;
                } else if (event.position - pressed).magnitude() > f64::from(f32::from(DRAG_START))
                {
                    // The system has the mouse until its button is up.
                    this.window_press = None;
                    window.start_window_move();
                    #[cfg(test)]
                    {
                        this.window_moves += 1;
                    }
                }
            }))
    }
}
