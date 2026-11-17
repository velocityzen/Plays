use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, FocusHandle, SharedString, Window, div, px};

use super::components::{Button, Segmented, Slide, Slider, Switch};
use super::theme::Theme;
use crate::app::AudioEngine;
use crate::settings::{ControlsVisibility, MaxResolution, Settings, ThemeMode, VideoFit};

/// Change the settings, re-render every window and optionally write them to disk.
fn update_settings(cx: &mut App, save: bool, change: impl FnOnce(&mut Settings)) {
    cx.update_global::<Settings, _>(|settings, _| {
        change(settings);
        if save {
            settings.save();
        }
    });
    cx.refresh_windows();
}

pub struct SettingsView {
    focus_handle: FocusHandle,
    master_drag: Rc<Cell<bool>>,
    default_drag: Rc<Cell<bool>>,
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        Self {
            focus_handle,
            master_drag: Rc::default(),
            default_drag: Rc::default(),
        }
    }
}

fn section(title: &'static str, theme: &Theme, rows: Vec<AnyElement>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_muted)
                .child(title.to_uppercase()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(10.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    div()
                        .when(index > 0, |this| {
                            this.border_t_1().border_color(theme.border)
                        })
                        .child(row)
                })),
        )
}

fn row(
    label: &'static str,
    hint: Option<SharedString>,
    theme: &Theme,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_3()
        .py_2p5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .child(div().text_sm().child(label))
                .when_some(hint, |this, hint| {
                    this.child(div().text_xs().text_color(theme.text_muted).child(hint))
                }),
        )
        .child(control)
        .into_any_element()
}

fn percent_slider(
    id: &'static str,
    value: f32,
    dragging: Rc<Cell<bool>>,
    theme: &Theme,
    apply: impl Fn(&mut Settings, f32) + Copy + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .w(px(200.))
        .child(
            Slider::new(id, value, dragging).on_change(move |event, _, cx| {
                let value = (event.value * 100.0).round() / 100.0;
                update_settings(cx, event.slide == Slide::End, |s| apply(s, value));
            }),
        )
        .child(
            div()
                .w(px(40.))
                .flex_none()
                .text_right()
                .text_sm()
                .text_color(theme.text_muted)
                .child(format!("{}%", (value * 100.0).round())),
        )
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let s = cx.global::<Settings>().clone();
        let device = AudioEngine::device_description(cx);
        let path_hint = Settings::path()
            .map(|p| format!("Saved to {}", p.display()))
            .unwrap_or_else(|| "Settings can't be saved on this system".into());

        let playback = vec![
            row(
                "Play files when opened",
                None,
                &theme,
                Switch::new("autoplay", s.autoplay)
                    .on_toggle(|&on, _, cx| update_settings(cx, true, |s| s.autoplay = on)),
            ),
            row(
                "Loop new files",
                None,
                &theme,
                Switch::new("loop", s.loop_by_default)
                    .on_toggle(|&on, _, cx| update_settings(cx, true, |s| s.loop_by_default = on)),
            ),
            row(
                "Volume for new files",
                None,
                &theme,
                percent_slider(
                    "default-volume",
                    s.default_volume,
                    self.default_drag.clone(),
                    &theme,
                    |s, v| s.default_volume = v,
                ),
            ),
            row(
                "Seek step",
                Some("Left and right arrow keys move every file by this much".into()),
                &theme,
                Segmented::new(
                    "seek-step",
                    &[(5, "5 s"), (10, "10 s"), (30, "30 s"), (60, "60 s")],
                    s.seek_step_seconds,
                )
                .on_select(|&v, _, cx| update_settings(cx, true, |s| s.seek_step_seconds = v)),
            ),
        ];

        let audio = vec![
            row(
                "Master volume",
                None,
                &theme,
                percent_slider(
                    "master-volume",
                    s.master_volume,
                    self.master_drag.clone(),
                    &theme,
                    |s, v| s.master_volume = v,
                ),
            ),
            row(
                "Output",
                None,
                &theme,
                div()
                    .text_sm()
                    .text_color(theme.text_muted)
                    .max_w(px(260.))
                    .truncate()
                    .child(device),
            ),
        ];

        let video = vec![
            row(
                "Scaling",
                None,
                &theme,
                Segmented::new(
                    "fit",
                    &[(VideoFit::Contain, "Fit"), (VideoFit::Cover, "Fill")],
                    s.video_fit,
                )
                .on_select(|&v, _, cx| update_settings(cx, true, |s| s.video_fit = v)),
            ),
            row(
                "Decode resolution limit",
                Some("For files opened from now on. Lower limits save CPU and memory with many videos.".into()),
                &theme,
                Segmented::new(
                    "max-resolution",
                    &[
                        (MaxResolution::Original, "Full"),
                        (MaxResolution::P2160, "2160p"),
                        (MaxResolution::P1080, "1080p"),
                        (MaxResolution::P720, "720p"),
                        (MaxResolution::P480, "480p"),
                    ],
                    s.max_resolution,
                )
                .on_select(|&v, _, cx| update_settings(cx, true, |s| s.max_resolution = v)),
            ),
            row(
                "Hardware decoding",
                Some(
                    if cfg!(target_os = "macos") {
                        "Decode on the GPU with VideoToolbox when it supports the video. For files opened from now on."
                    } else {
                        "Decode on the GPU with VA-API, NVDEC or VDPAU when one supports the video. For files opened from now on."
                    }
                    .into(),
                ),
                &theme,
                Switch::new("hardware", s.hardware_decoding)
                    .on_toggle(|&on, _, cx| update_settings(cx, true, |s| s.hardware_decoding = on)),
            ),
        ];

        let layout = vec![
            row(
                "Theme",
                None,
                &theme,
                Segmented::new(
                    "theme",
                    &[
                        (ThemeMode::System, "System"),
                        (ThemeMode::Dark, "Dark"),
                        (ThemeMode::Light, "Light"),
                    ],
                    s.theme,
                )
                .on_select(|&v, _, cx| update_settings(cx, true, |s| s.theme = v)),
            ),
            row(
                "Columns",
                None,
                &theme,
                Segmented::new(
                    "columns",
                    &[(0, "Auto"), (1, "1"), (2, "2"), (3, "3"), (4, "4")],
                    s.columns,
                )
                .on_select(|&v, _, cx| update_settings(cx, true, |s| s.columns = v)),
            ),
            row(
                "Player controls",
                None,
                &theme,
                Segmented::new(
                    "controls",
                    &[
                        (ControlsVisibility::OnHover, "On hover"),
                        (ControlsVisibility::Always, "Always"),
                    ],
                    s.controls,
                )
                .on_select(|&v, _, cx| update_settings(cx, true, |s| s.controls = v)),
            ),
            row(
                "Show file names",
                None,
                &theme,
                Switch::new("titles", s.show_titles)
                    .on_toggle(|&on, _, cx| update_settings(cx, true, |s| s.show_titles = on)),
            ),
        ];

        div()
            .id("settings")
            .key_context("Settings")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .px_5()
                    .pt_4()
                    .pb_5()
                    .child(section("Playback", &theme, playback))
                    .child(section("Audio", &theme, audio))
                    .child(section("Video", &theme, video))
                    .child(section("Layout", &theme, layout))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(theme.text_muted)
                                    .child(path_hint),
                            )
                            .child(Button::new("reset", "Reset to defaults").on_click(
                                |_, _, cx| update_settings(cx, true, |s| *s = Settings::default()),
                            )),
                    ),
            )
    }
}
