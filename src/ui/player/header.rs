//! The bar over the top of a block: the file's name, where it is in its
//! playlist and what it is, and buttons for Picture in Picture, to pop the
//! block out, expand it and close it.

use gpui::prelude::*;
use gpui::{Context, Div, Window, div, linear_color_stop, linear_gradient};
use serde_json::Value;

use super::{PlayerView, Reveal};
use crate::media::decoder::MediaInfo;
use crate::ui::components::Button;
use crate::ui::pip;
use crate::ui::theme::layout::TRAFFIC_LIGHTS_WIDTH;
use crate::ui::theme::overlay;

impl PlayerView {
    pub(super) fn render_header(
        &self,
        reveal: &Reveal,
        info: Option<&MediaInfo>,
        show_titles: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let expanded = if self.layout.alone {
            window.is_fullscreen()
        } else {
            self.layout.maximized
        };
        // Full screen hides the traffic lights.
        let under_traffic_lights =
            cfg!(target_os = "macos") && self.layout.top_left && !window.is_fullscreen();

        reveal
            .apply(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl_3()
                    // As far from the right edge as from the top, which
                    // lines the buttons up with the traffic lights.
                    .pr_2()
                    .pt_2()
                    .pb_4()
                    .when(under_traffic_lights, |this| this.pl(TRAFFIC_LIGHTS_WIDTH))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(overlay::scrim(), 0.),
                        linear_color_stop(overlay::scrim_clear(), 1.),
                    )),
            )
            .child(self.render_names(info, show_titles))
            .when(
                pip::supported()
                    && self.playback.local().frame().is_some()
                    && !self.casts_picture(),
                |this| {
                    this.child(
                        Button::icon("pip", "pip")
                            .overlay()
                            .selected(self.pip.is_some())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.send("block.picture_in_picture.toggle", Value::Null, cx);
                            })),
                    )
                },
            )
            .when(!self.layout.alone, |this| {
                this.child(
                    Button::icon("pop-out", "pop-out")
                        .overlay()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.send("block.move_to_new_window", Value::Null, cx);
                        })),
                )
            })
            .child(
                Button::icon("maximize", if expanded { "minimize" } else { "maximize" })
                    .overlay()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send("block.zoom", Value::Null, cx);
                    })),
            )
            .child(
                Button::icon("close", "close")
                    .overlay()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send("block.close", Value::Null, cx);
                    })),
            )
    }

    /// What the header says it plays: its title (or label, which shows
    /// though titles don't, as it was given), where that is in its
    /// playlist, and what it is.
    fn render_names(&self, info: Option<&MediaInfo>, show_titles: bool) -> Div {
        let aside = |text: String| {
            div()
                .flex_none()
                .text_xs()
                .text_color(overlay::text_muted())
                .child(text)
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_baseline()
            .gap_2()
            .when(show_titles || self.label.is_some(), |this| {
                this.child(
                    div()
                        .debug_selector(|| "block-title".into())
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(overlay::text())
                        .child(self.header_title()),
                )
            })
            .when_some(self.playlist_counter(), |this, counter| {
                this.child(aside(counter).debug_selector(|| "block-playlist".into()))
            })
            .when_some(
                info.map(details).filter(|details| !details.is_empty()),
                |this, details| this.child(aside(details)),
            )
    }
}

/// What the file is: the picture's size, its frame rate, and the codecs.
fn details(info: &MediaInfo) -> String {
    let mut parts = Vec::new();
    if let Some((w, h)) = info.upright_size() {
        parts.push(format!("{w}×{h}"));
    }
    if let Some(fps) = info.frame_rate {
        parts.push(format!("{} fps", (fps * 100.0).round() / 100.0));
    }
    parts.extend(info.video_codec.as_ref().map(|codec| match info.hardware {
        Some(api) => format!("{codec} ({api})"),
        None => codec.clone(),
    }));
    parts.extend(info.audio_codec.clone());
    parts.join(" · ")
}
