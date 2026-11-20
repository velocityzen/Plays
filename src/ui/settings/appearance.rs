//! Appearance: the theme, and how windows lay out their tiles.

use gpui::{AnyElement, Context, Window};

use super::form::{group, row, update_settings};
use super::{Pane, SettingsView};
use crate::settings::{ControlsVisibility, FloatOnTop, FullScreen, PanelLook, Settings, ThemeMode};
use crate::ui::components::{Segmented, Switch};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::APPEARANCE_COLOR;

pub(super) struct AppearancePane;

impl Pane for AppearancePane {
    fn name(&self) -> &'static str {
        "Appearance"
    }

    fn icon(&self) -> &'static str {
        "appearance"
    }

    fn color(&self) -> u32 {
        APPEARANCE_COLOR
    }

    fn content(
        &self,
        _: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        _: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        vec![
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
                        "Panels over the video",
                        Some(
                            "How the controls, notes and menus on a video show it through them"
                                .into(),
                        ),
                        theme,
                        Segmented::new(
                            "panels",
                            &[
                                (PanelLook::Plain, "Plain"),
                                (PanelLook::Frosted, "Frosted"),
                                (PanelLook::Glass, "Glass"),
                                (PanelLook::Glow, "Glow"),
                                (PanelLook::Tint, "Tint"),
                            ],
                            s.panels,
                        )
                        .on_select(|&v, _, cx| update_settings(cx, true, |s| s.panels = v)),
                    ),
                    row(
                        "Show file names",
                        None,
                        theme,
                        Switch::new("titles", s.show_titles).on_toggle(|&on, _, cx| {
                            update_settings(cx, true, |s| s.show_titles = on)
                        }),
                    ),
                    row(
                        "Float on top",
                        None,
                        theme,
                        Segmented::new(
                            "float-on-top",
                            &[
                                (FloatOnTop::Off, "Off"),
                                (FloatOnTop::Always, "Always"),
                                (FloatOnTop::WhilePlaying, "While playing"),
                            ],
                            s.float_on_top,
                        )
                        .on_select(|&v, _, cx| update_settings(cx, true, |s| s.float_on_top = v)),
                    ),
                ]
                .into_iter()
                .chain(cfg!(target_os = "macos").then(|| full_screen_row(s, theme)))
                .collect(),
            ),
        ]
    }
}

/// Full screen in a Space of its own, as macOS has it, or over the display
/// the window is on.
fn full_screen_row(s: &Settings, theme: &Theme) -> AnyElement {
    row(
        "Full screen",
        Some(
            "Over the display leaves the other displays as they are, and goes in \
             and out at once."
                .into(),
        ),
        theme,
        Segmented::new(
            "full-screen",
            &[
                (FullScreen::OwnSpace, "Own Space"),
                (FullScreen::OverDisplay, "Over the display"),
            ],
            s.full_screen,
        )
        .on_select(|&v, _, cx| update_settings(cx, true, |s| s.full_screen = v)),
    )
}
