//! Appearance: the theme, how windows lay out their tiles, which panels
//! show over the video, and the empty window's screensaver.

use gpui::{AnyElement, Context, Window};

use super::form::{group, row, set};
use super::{Pane, SettingsView};
use crate::settings::{Settings, fields};
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
                    Segmented::new("theme", &fields::THEMES, s.theme)
                        .on_select(|&v, _, cx| set("theme", v, cx)),
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
                        Segmented::new("columns", &fields::COLUMNS, s.columns)
                            .on_select(|&v, _, cx| set("columns", v, cx)),
                    ),
                    row(
                        "Player controls",
                        None,
                        theme,
                        Segmented::new("controls", &fields::CONTROLS, s.controls)
                            .on_select(|&v, _, cx| set("controls", v, cx)),
                    ),
                    row(
                        "Panels over the video",
                        Some(
                            "How the controls, notes and menus on a video show it through them"
                                .into(),
                        ),
                        theme,
                        Segmented::new("panels", &fields::PANELS, s.panels)
                            .on_select(|&v, _, cx| set("panels", v, cx)),
                    ),
                    row(
                        "Show file names",
                        None,
                        theme,
                        Switch::new("titles", s.show_titles)
                            .on_toggle(|&on, _, cx| set("show_titles", on, cx)),
                    ),
                    row(
                        "Float on top",
                        None,
                        theme,
                        Segmented::new("float-on-top", &fields::FLOATING, s.float_on_top)
                            .on_select(|&v, _, cx| set("float_on_top", v, cx)),
                    ),
                ]
                .into_iter()
                .chain(cfg!(target_os = "macos").then(|| full_screen_row(s, theme)))
                .chain([black_out_row(s, theme)])
                .collect(),
            ),
            group(Some("Over the video"), theme, over_video_rows(s, theme)),
            group(Some("Empty window"), theme, screensaver_rows(s, theme)),
        ]
    }
}

/// Each panel over the video on or off: its name, what it is, and its
/// setting, which is its switch's id.
fn over_video_rows(s: &Settings, theme: &Theme) -> Vec<AnyElement> {
    let shown = s.over_video;
    [
        (
            "Controls",
            "The seek bar and buttons at the bottom.",
            "over_video.controls",
            shown.controls,
        ),
        (
            "Top bar",
            "The file's name and buttons at the top.",
            "over_video.top_bar",
            shown.top_bar,
        ),
        (
            "Track switcher",
            "The reel a shortcut rolls to the audio, video or subtitles picked.",
            "over_video.track_switcher",
            shown.track_switcher,
        ),
        (
            "Notes",
            "What a shortcut changed, like the volume. Notes on what's wrong still show.",
            "over_video.notes",
            shown.notes,
        ),
    ]
    .into_iter()
    .map(|(name, hint, key, on)| {
        row(
            name,
            Some(hint.into()),
            theme,
            Switch::new(key, on).on_toggle(move |&on, _, cx| set(key, on, cx)),
        )
    })
    .collect()
}

/// The screensaver on or off and, on, how long the window rests first.
fn screensaver_rows(s: &Settings, theme: &Theme) -> Vec<AnyElement> {
    let switch = row(
        "Screensaver",
        Some(
            "Left alone in front, the window fades its buttons out and bounces its \
             logo off its edges."
                .into(),
        ),
        theme,
        Switch::new("screensaver", s.screensaver)
            .on_toggle(|&on, _, cx| set("screensaver", on, cx)),
    );
    let after = s.screensaver.then(|| {
        row(
            "Start after",
            None,
            theme,
            Segmented::new(
                "screensaver-minutes",
                &fields::SCREENSAVER_MINUTES,
                s.screensaver_minutes,
            )
            .on_select(|&v, _, cx| set("screensaver_minutes", v, cx)),
        )
    });
    [switch].into_iter().chain(after).collect()
}

/// The other displays in black while a window is full screen.
fn black_out_row(s: &Settings, theme: &Theme) -> AnyElement {
    row(
        "Black out other displays",
        Some(
            "While a window is full screen. Moving the mouse onto one, or \
             clicking it, shows it until the mouse rests there."
                .into(),
        ),
        theme,
        Switch::new("black-out", s.black_out_displays)
            .on_toggle(|&on, _, cx| set("black_out_displays", on, cx)),
    )
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
        Segmented::new("full-screen", &fields::FULL_SCREENS, s.full_screen)
            .on_select(|&v, _, cx| set("full_screen", v, cx)),
    )
}
