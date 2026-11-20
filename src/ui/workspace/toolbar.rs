//! The toolbar, where there is no menu bar: the open buttons, what is open,
//! and the window's buttons when the app draws its own decorations.

use gpui::prelude::*;
use gpui::{AnyElement, App, ClickEvent, Context, Decorations, MouseButton, Window, div};

use super::Workspace;
use crate::app;
use crate::ui::components::Button;
use crate::ui::theme::Theme;
use crate::ui::theme::layout::{COMPACT_WIDTH, TOOLBAR_HEIGHT};

impl Workspace {
    pub(super) fn render_toolbar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        theme: &Theme,
    ) -> impl IntoElement {
        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let count = self.tiles.len();
        let playing = self.any_playing(cx);
        let compact = window.viewport_size().width < COMPACT_WIDTH;
        let status = self.status(cx);

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(TOOLBAR_HEIGHT)
            .px_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .child(tool(
                compact,
                "open",
                "plus",
                "Open…",
                cx.listener(|this, _, window, cx| this.prompt_open(window, cx)),
            ))
            .child(tool(
                compact,
                "open-url",
                "link",
                "Open URL…",
                cx.listener(|this, _, window, cx| this.show_open_url(window, cx)),
            ))
            .when(count > 0, |this| {
                this.child(tool(
                    compact,
                    "play-pause-all",
                    if playing { "pause" } else { "play" },
                    if playing { "Pause all" } else { "Play all" },
                    cx.listener(|this, _, _, cx| this.toggle_all(cx)),
                ))
                .child(tool(
                    compact,
                    "restart-all",
                    "replay",
                    "Restart all",
                    cx.listener(|this, _, _, cx| this.restart_all(cx)),
                ))
                .child(tool(
                    compact,
                    "close-all",
                    "close",
                    "Close all",
                    cx.listener(|this, _, _, cx| this.close_all(cx)),
                ))
            })
            .child(
                // Empty toolbar space moves the window when we draw our own decorations.
                div()
                    .id("drag-area")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px_2()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .on_mouse_down(MouseButton::Left, move |_, window, _| {
                        if client_decorated {
                            window.start_window_move();
                        }
                    })
                    .child(status),
            )
            .child(
                Button::icon("settings", "settings")
                    .icon_color(theme.text_muted)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(app::OpenSettings), cx)
                    }),
            )
            .when(client_decorated, |this| {
                this.child(
                    Button::icon("minimize-window", "window-min")
                        .icon_color(theme.text_muted)
                        .on_click(|_, window, _| window.minimize_window()),
                )
                .child(
                    Button::icon("zoom-window", "window-max")
                        .icon_color(theme.text_muted)
                        .on_click(|_, window, _| window.zoom_window()),
                )
                .child(
                    Button::icon("close-window", "close")
                        .icon_color(theme.text_muted)
                        .on_click(|_, window, _| window.remove_window()),
                )
            })
    }

    /// What is open, as the toolbar says it: how many files and streams.
    fn status(&self, cx: &App) -> String {
        let count = self.tiles.len();
        let streams = self
            .tiles
            .iter()
            .filter(|t| t.view.read(cx).player().is_network())
            .count();
        let plural = |n: usize, one: &str, many: &str| match n {
            1 => format!("1 {one}"),
            n => format!("{n} {many}"),
        };
        match (count - streams, streams) {
            (0, 0) => "Nothing open".to_string(),
            (files, 0) => plural(files, "file", "files"),
            (0, streams) => plural(streams, "stream", "streams"),
            (files, streams) => format!(
                "{} · {}",
                plural(files, "file", "files"),
                plural(streams, "stream", "streams")
            ),
        }
    }
}

/// A toolbar button. Narrow windows show just its icon so the toolbar still fits.
fn tool(
    compact: bool,
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    if compact {
        Button::icon(id, icon).on_click(on_click).into_any_element()
    } else {
        Button::new(id, label)
            .icon_left(icon)
            .on_click(on_click)
            .into_any_element()
    }
}
