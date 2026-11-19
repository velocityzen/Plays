//! The empty window: a ring like the one in the app's icon, with the open
//! buttons inside, or the "Open URL" form while it is open.

use gpui::prelude::*;
use gpui::{Context, ExternalPaths, Window, div};

use super::{CONTENT_GROUP, TOOLBAR_SPACE, Workspace};
use crate::ui::components::Button;
use crate::ui::theme::Theme;
use crate::ui::theme::layout::{
    RING_BORDER, RING_BORDER_DROP, RING_FORM_MAX_WIDTH, RING_FORM_MIN_WIDTH, RING_FORM_SCALE,
    RING_MARGIN, RING_MAX_DIAMETER, RING_MIN_DIAMETER, RING_SCALE,
};
use crate::ui::theme::text::TextStyle;

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
            .flex_col()
            .justify_center()
            .text_center()
            // The title sits in the very middle, with the rest below it.
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(div().text_display().child("It plays for you"))
                    .child(
                        div()
                            .absolute()
                            .top_full()
                            .left_0()
                            .right_0()
                            .pt_4()
                            .flex()
                            .justify_center()
                            .map(|this| {
                                if self.url_form_open {
                                    this.child(div().w(form_width).child(self.url_form.clone()))
                                } else {
                                    this.gap_2()
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
                                        )
                                }
                            }),
                    ),
            );

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(ring)
    }
}
