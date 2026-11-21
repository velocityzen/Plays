//! The empty window: the app's logo, with the open buttons under it, or
//! the "Open URL" form while it is open. Pressed anywhere else and dragged,
//! it moves the window, as its title bar does. Left alone, its logo
//! bounces about (`screensaver`).

use gpui::prelude::*;
use gpui::{
    Context, Div, ExternalPaths, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Window,
    div, svg,
};

use serde_json::Value;

use super::screensaver::{measure, note_home, note_room};
use super::{CONTENT_GROUP, TOOLBAR_SPACE, Workspace};
use crate::assets::LOGO;
use crate::ui::components::Button;
use crate::ui::player::move_window_once_dragged;
use crate::ui::theme::Theme;
use crate::ui::theme::layout::{
    EMPTY_GAP, EMPTY_WIDTH, LOGO_DROP_OPACITY, LOGO_MAX, LOGO_OPACITY, LOGO_SCALE,
};

impl Workspace {
    /// The app's logo, as large as the window allows, with the open
    /// buttons under it, side by side and alike.
    pub(super) fn render_empty(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (open_shortcut, url_shortcut) = if cfg!(target_os = "macos") {
            ("⌘O", "⇧⌘O")
        } else {
            ("Ctrl+O", "Shift+Ctrl+O")
        };
        let viewport = window.viewport_size();
        let shorter = viewport.width.min(viewport.height - TOOLBAR_SPACE);
        let accent = theme.accent;
        let size = (shorter * LOGO_SCALE).min(LOGO_MAX);
        let frame = self.screensaver_frame(size, window, cx);
        let this = cx.entity().downgrade();

        let logo = svg()
            .path(LOGO)
            .id("logo")
            .debug_selector(|| "logo".into())
            .flex_none()
            .size(size)
            .text_color(
                frame
                    .color
                    .unwrap_or_else(|| theme.text.opacity(LOGO_OPACITY)),
            )
            .group_drag_over::<ExternalPaths>(CONTENT_GROUP, move |style| {
                style.text_color(accent.opacity(LOGO_DROP_OPACITY))
            })
            // Clicked in its place, it takes its next color; out, a click
            // only brings it back.
            .when(frame.logo_at.is_none(), |logo| {
                logo.on_click(cx.listener(|this, _, _, cx| this.logo_clicked(cx)))
            });
        // Its place keeps the buttons where they are while it's out of it.
        let place = div()
            .relative()
            .flex_none()
            .size(size)
            .child(measure(this.clone(), note_home));
        let (place, out) = match frame.logo_at {
            None => (place.child(logo), None),
            Some(at) => (place, Some(logo.absolute().left(at.x).top(at.y))),
        };

        // A press on what's under it is its own, not the window's. Faded
        // out, it takes none.
        let under = div()
            .w(EMPTY_WIDTH)
            .opacity(frame.buttons)
            .when(frame.buttons <= 0., Styled::invisible)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        let under = if self.url_form_open {
            under.child(self.url_form.clone())
        } else {
            under
                .flex()
                .gap_2()
                .child(
                    Button::new("open-empty", "Open")
                        .wide()
                        .shortcut(open_shortcut)
                        .flex_1()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.send("window.open_files", Value::Null, cx);
                        })),
                )
                .child(
                    Button::new("open-url-empty", "Open URL")
                        .wide()
                        .shortcut(url_shortcut)
                        .flex_1()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.send("window.open_url", Value::Null, cx);
                        })),
                )
        };

        let empty = div()
            .debug_selector(|| "empty".into())
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(EMPTY_GAP);
        self.move_window_by(empty, window, cx)
            .child(measure(this, note_room))
            .child(place)
            .child(under)
            .children(out)
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
