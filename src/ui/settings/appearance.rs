//! Appearance: the theme, and how windows lay out their tiles.

use gpui::{AnyElement, Context, Window};

use super::form::{group, row, update_settings};
use super::{Pane, SettingsView};
use crate::settings::{ControlsVisibility, Settings, ThemeMode};
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
                        "Show file names",
                        None,
                        theme,
                        Switch::new("titles", s.show_titles).on_toggle(|&on, _, cx| {
                            update_settings(cx, true, |s| s.show_titles = on)
                        }),
                    ),
                ],
            ),
        ]
    }
}
