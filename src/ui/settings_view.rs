use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, FocusHandle, FontWeight, Hsla, SharedString, Window, div, px, rgb,
    white,
};

use super::components::{Button, Icon, Segmented, Slide, Slider, Switch};
use super::theme::Theme;
use crate::app::AudioEngine;
use crate::settings::{
    ControlsVisibility, GAMMA_RANGE, MaxResolution, Settings, ThemeMode, VideoFit,
};

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

/// The settings window's sections: a sidebar entry and a pane each.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Playback,
    Audio,
    Video,
    Appearance,
}

impl Section {
    const ALL: [Self; 4] = [Self::Playback, Self::Audio, Self::Video, Self::Appearance];

    fn title(self) -> &'static str {
        match self {
            Self::Playback => "Playback",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Appearance => "Appearance",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Playback => "play",
            Self::Audio => "volume",
            Self::Video => "display",
            Self::Appearance => "appearance",
        }
    }

    /// The badge behind the icon, in macOS's system colors, as System
    /// Settings has them.
    fn color(self) -> Hsla {
        match self {
            Self::Playback => rgb(0x34c759),
            Self::Audio => rgb(0xff375f),
            Self::Video => rgb(0x0a84ff),
            Self::Appearance => rgb(0x8e8e93),
        }
        .into()
    }
}

pub struct SettingsView {
    focus_handle: FocusHandle,
    section: Section,
    master_drag: Rc<Cell<bool>>,
    default_drag: Rc<Cell<bool>>,
    sdr_gamma_drag: Rc<Cell<bool>>,
    hdr_gamma_drag: Rc<Cell<bool>>,
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        Self {
            focus_handle,
            section: Section::Playback,
            master_drag: Rc::default(),
            default_drag: Rc::default(),
            sdr_gamma_drag: Rc::default(),
            hdr_gamma_drag: Rc::default(),
        }
    }
}

/// Rows on one card, under an optional heading.
fn group(title: Option<&'static str>, theme: &Theme, rows: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .when_some(title, |this, title| {
            this.child(
                div()
                    .px_1()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
        })
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
        .into_any_element()
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

/// A gamma slider. Gamma is spread evenly by its logarithm, so 1 sits in the
/// middle, and moves in steps of 0.05, so it comes back to exactly 1.
fn gamma_slider(
    id: &'static str,
    value: f32,
    dragging: Rc<Cell<bool>>,
    theme: &Theme,
    apply: impl Fn(&mut Settings, f32) + Copy + 'static,
) -> impl IntoElement {
    let (low, high) = (GAMMA_RANGE.start().log2(), GAMMA_RANGE.end().log2());
    let position = (value.log2() - low) / (high - low);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .w(px(200.))
        .child(
            Slider::new(id, position, dragging).on_change(move |event, _, cx| {
                let gamma = (low + event.value * (high - low)).exp2();
                let gamma = (gamma * 20.0).round() / 20.0;
                update_settings(cx, event.slide == Slide::End, |s| apply(s, gamma));
            }),
        )
        .child(
            div()
                .w(px(40.))
                .flex_none()
                .text_right()
                .text_sm()
                .text_color(theme.text_muted)
                .child(format!("{value:.2}")),
        )
}

impl SettingsView {
    /// A section's pane: its groups of rows.
    fn groups(&self, section: Section, theme: &Theme, cx: &App) -> Vec<AnyElement> {
        let s = cx.global::<Settings>().clone();
        match section {
            Section::Playback => vec![group(
                None,
                theme,
                vec![
                    row(
                        "Play files when opened",
                        None,
                        theme,
                        Switch::new("autoplay", s.autoplay)
                            .on_toggle(|&on, _, cx| update_settings(cx, true, |s| s.autoplay = on)),
                    ),
                    row(
                        "Loop new files",
                        None,
                        theme,
                        Switch::new("loop", s.loop_by_default).on_toggle(|&on, _, cx| {
                            update_settings(cx, true, |s| s.loop_by_default = on)
                        }),
                    ),
                    row(
                        "Volume for new files",
                        None,
                        theme,
                        percent_slider(
                            "default-volume",
                            s.default_volume,
                            self.default_drag.clone(),
                            theme,
                            |s, v| s.default_volume = v,
                        ),
                    ),
                    row(
                        "Seek step",
                        Some("Left and right arrow keys move every file by this much".into()),
                        theme,
                        Segmented::new(
                            "seek-step",
                            &[(5, "5 s"), (10, "10 s"), (30, "30 s"), (60, "60 s")],
                            s.seek_step_seconds,
                        )
                        .on_select(|&v, _, cx| {
                            update_settings(cx, true, |s| s.seek_step_seconds = v)
                        }),
                    ),
                ],
            )],
            Section::Audio => vec![group(
                None,
                theme,
                vec![
                    row(
                        "Master volume",
                        None,
                        theme,
                        percent_slider(
                            "master-volume",
                            s.master_volume,
                            self.master_drag.clone(),
                            theme,
                            |s, v| s.master_volume = v,
                        ),
                    ),
                    row(
                        "Output",
                        None,
                        theme,
                        div()
                            .text_sm()
                            .text_color(theme.text_muted)
                            .max_w(px(260.))
                            .truncate()
                            .child(AudioEngine::device_description(cx)),
                    ),
                ],
            )],
            Section::Video => {
                let [sdr_gamma, hdr_gamma] = s.gamma();
                vec![
                    group(
                        Some("Picture"),
                        theme,
                        vec![
                            row(
                                "Scaling",
                                None,
                                theme,
                                Segmented::new(
                                    "fit",
                                    &[(VideoFit::Contain, "Fit"), (VideoFit::Cover, "Fill")],
                                    s.video_fit,
                                )
                                .on_select(|&v, _, cx| {
                                    update_settings(cx, true, |s| s.video_fit = v)
                                }),
                            ),
                            row(
                                "SDR gamma",
                                Some(
                                    "Above 1 brightens shadows and midtones, below 1 darkens \
                                     them. For every open video."
                                        .into(),
                                ),
                                theme,
                                gamma_slider(
                                    "sdr-gamma",
                                    sdr_gamma,
                                    self.sdr_gamma_drag.clone(),
                                    theme,
                                    |s, v| s.sdr_gamma = v,
                                ),
                            ),
                            row(
                                "HDR gamma",
                                Some("The same for HDR10, HLG and Dolby Vision video.".into()),
                                theme,
                                gamma_slider(
                                    "hdr-gamma",
                                    hdr_gamma,
                                    self.hdr_gamma_drag.clone(),
                                    theme,
                                    |s, v| s.hdr_gamma = v,
                                ),
                            ),
                        ],
                    ),
                    group(
                        Some("Decoding"),
                        theme,
                        vec![
                            row(
                                "Resolution limit",
                                Some(
                                    "For files opened from now on. Lower limits save CPU and \
                                     memory with many videos."
                                        .into(),
                                ),
                                theme,
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
                                .on_select(|&v, _, cx| {
                                    update_settings(cx, true, |s| s.max_resolution = v)
                                }),
                            ),
                            row(
                                "Hardware decoding",
                                Some(
                                    if cfg!(target_os = "macos") {
                                        "Decode on the GPU with VideoToolbox when it supports \
                                         the video. For files opened from now on."
                                    } else {
                                        "Decode on the GPU with VA-API, NVDEC or VDPAU when one \
                                         supports the video. For files opened from now on."
                                    }
                                    .into(),
                                ),
                                theme,
                                Switch::new("hardware", s.hardware_decoding).on_toggle(
                                    |&on, _, cx| {
                                        update_settings(cx, true, |s| s.hardware_decoding = on)
                                    },
                                ),
                            ),
                        ],
                    ),
                ]
            }
            Section::Appearance => vec![
                group(
                    None,
                    theme,
                    vec![row(
                        "Theme",
                        None,
                        theme,
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
                    )],
                ),
                group(
                    Some("Windows"),
                    theme,
                    vec![
                        row(
                            "Columns",
                            None,
                            theme,
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
                            theme,
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
                            theme,
                            Switch::new("titles", s.show_titles).on_toggle(|&on, _, cx| {
                                update_settings(cx, true, |s| s.show_titles = on)
                            }),
                        ),
                    ],
                ),
            ],
        }
    }

    fn render_sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .w(px(200.))
            .h_full()
            .flex()
            .flex_col()
            .gap_0p5()
            .px_2p5()
            // Below the traffic lights, which sit over the sidebar on macOS.
            .pt(if cfg!(target_os = "macos") {
                px(48.)
            } else {
                px(10.)
            })
            .bg(theme.tile)
            .border_r_1()
            .border_color(theme.border)
            .children(Section::ALL.map(|section| {
                let selected = section == self.section;
                div()
                    .id(section.title())
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(30.))
                    .px_1p5()
                    .rounded(px(7.))
                    .text_sm()
                    .map(|this| {
                        if selected {
                            this.bg(theme.accent).text_color(white())
                        } else {
                            this.hover(|style| style.bg(theme.surface_hover))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.section = section;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_none()
                            .size(px(20.))
                            .rounded(px(5.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(section.color())
                            .child(Icon::new(section.icon()).size(px(12.)).color(white())),
                    )
                    .child(section.title())
            }))
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let path_hint = Settings::path()
            .map(|p| format!("Saved to {}", p.display()))
            .unwrap_or_else(|| "Settings can't be saved on this system".into());
        let section = self.section;

        let pane = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                // In line with the traffic lights on macOS, like a title.
                div()
                    .flex_none()
                    .h(px(42.))
                    .px_5()
                    .flex()
                    .items_center()
                    .text_base()
                    .font_weight(FontWeight::BOLD)
                    .child(section.title()),
            )
            .child(
                div()
                    // Each pane scrolls on its own and opens at the top.
                    .id(("settings-pane", section as usize))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .px_5()
                    .pt_2()
                    .pb_5()
                    .children(self.groups(section, &theme, cx)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_5()
                    .py_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(path_hint),
                    )
                    .child(
                        Button::new("reset", "Reset to defaults").on_click(|_, _, cx| {
                            update_settings(cx, true, |s| *s = Settings::default())
                        }),
                    ),
            );

        div()
            .id("settings")
            .key_context("Settings")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(self.render_sidebar(&theme, cx))
            .child(pane)
    }
}
