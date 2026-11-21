//! The empty window: the app's logo, with the open buttons under it, or
//! the "Open URL" form while it is open. Pressed anywhere else and dragged,
//! it moves the window, as its title bar does.

use gpui::prelude::*;
use gpui::{
    Context, Div, ExternalPaths, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Window,
    div, svg,
};

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
        let accent = theme.accent;

        let logo = svg()
            .debug_selector(|| "logo".into())
            .path(LOGO)
            .flex_none()
            .size((shorter * LOGO_SCALE).min(LOGO_MAX))
            .text_color(theme.text.opacity(LOGO_OPACITY))
            .group_drag_over::<ExternalPaths>(CONTENT_GROUP, move |style| {
                style.text_color(accent.opacity(LOGO_DROP_OPACITY))
            });

        // A press on what's under it is its own, not the window's.
        let under = div()
            .w(EMPTY_WIDTH)
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
                        .on_click(cx.listener(|this, _, window, cx| this.prompt_open(window, cx))),
                )
                .child(
                    Button::new("open-url-empty", "Open URL")
                        .wide()
                        .shortcut(url_shortcut)
                        .flex_1()
                        .justify_center()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.show_open_url(window, cx)),
                        ),
                )
        };

        let empty = div()
            .debug_selector(|| "empty".into())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(EMPTY_GAP);
        self.move_window_by(empty, window, cx)
            .child(logo)
            .child(under)
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
