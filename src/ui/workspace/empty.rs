//! The empty window: a ring like the one in the app's icon, with the open
//! buttons inside, or the "Open URL" form while it is open. Pressed
//! anywhere else and dragged, it moves the window, as its title bar does.

use gpui::prelude::*;
use gpui::{
    Context, Div, ExternalPaths, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Window,
    div,
};

use super::{CONTENT_GROUP, TOOLBAR_SPACE, Workspace};
use crate::ui::components::Button;
use crate::ui::player::move_window_once_dragged;
use crate::ui::theme::Theme;
use crate::ui::theme::layout::{
    RING_BORDER, RING_BORDER_DROP, RING_FORM_MAX_WIDTH, RING_FORM_MIN_WIDTH, RING_FORM_SCALE,
    RING_MARGIN, RING_MAX_DIAMETER, RING_MIN_DIAMETER, RING_SCALE,
};

impl Workspace {
    /// A ring like the one in the app's icon, as large as the window allows,
    /// with the open buttons inside.
    pub(super) fn render_empty(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (open_shortcut, url_shortcut) = if cfg!(target_os = "macos") {
            ("⌘O", "⇧⌘O")
        } else {
            ("Ctrl+O", "Shift+Ctrl+O")
        };
        let viewport = window.viewport_size();
        let shorter = viewport.width.min(viewport.height - TOOLBAR_SPACE);
        // Small windows get a larger share, so what is inside still fits.
        let diameter = (shorter * RING_SCALE)
            .max(RING_MIN_DIAMETER.min(shorter - RING_MARGIN))
            .min(RING_MAX_DIAMETER);
        let form_width = (diameter * RING_FORM_SCALE)
            .max(RING_FORM_MIN_WIDTH)
            .min(RING_FORM_MAX_WIDTH);
        let accent = theme.accent;

        let ring = div()
            .flex_none()
            .size(diameter)
            .rounded_full()
            .border_2()
            .border_color(theme.text.opacity(RING_BORDER))
            .group_drag_over::<ExternalPaths>(CONTENT_GROUP, move |style| {
                style.border_color(accent.opacity(RING_BORDER_DROP))
            })
            .flex()
            .items_center()
            .justify_center()
            .map(|this| {
                // A press on what's inside is its own, not the window's.
                let inside =
                    div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
                if self.url_form_open {
                    this.child(inside.w(form_width).child(self.url_form.clone()))
                } else {
                    this.child(
                        inside
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("open-empty", "Open")
                                    .primary()
                                    .wide()
                                    .shortcut(open_shortcut)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.prompt_open(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("open-url-empty", "Open URL")
                                    .wide()
                                    .shortcut(url_shortcut)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_open_url(window, cx)
                                    })),
                            ),
                    )
                }
            });

        let empty = div()
            .debug_selector(|| "empty".into())
            .size_full()
            .flex()
            .items_center()
            .justify_center();
        self.move_window_by(empty, window, cx).child(ring)
    }

    /// Let a press on `empty`, dragged, move the window; not in full
    /// screen, where it stays put.
    fn move_window_by(&self, empty: Div, window: &Window, cx: &mut Context<Self>) -> Div {
        if window.is_fullscreen() {
            return empty;
        }
        empty
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    this.empty_press = Some(event.position);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.empty_press = None),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, _| {
                if move_window_once_dragged(&mut this.empty_press, event, window) {
                    #[cfg(test)]
                    {
                        this.window_moves += 1;
                    }
                }
            }))
    }
}
