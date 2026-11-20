//! Playback: what files do when they open, and how far seeking goes.

use gpui::{AnyElement, Context, Window};

use super::form::{group, percent_slider, row, update_settings};
use super::{Pane, SettingsView};
use crate::progress::{self, KEEP_DAYS};
use crate::settings::Settings;
use crate::ui::components::{Segmented, Switch};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::PLAYBACK_COLOR;

pub(super) struct PlaybackPane;

impl Pane for PlaybackPane {
    fn name(&self) -> &'static str {
        "Playback"
    }

    fn icon(&self) -> &'static str {
        "play"
    }

    fn color(&self) -> u32 {
        PLAYBACK_COLOR
    }

    fn content(
        &self,
        view: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        _: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        vec![group(
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
                    "Resume files where they were left",
                    Some(
                        format!(
                            "Each opens where it was, for {KEEP_DAYS} days since it last played"
                        )
                        .into(),
                    ),
                    theme,
                    Switch::new("resume", s.resume_files).on_toggle(|&on, _, cx| {
                        update_settings(cx, true, |s| s.resume_files = on);
                        // Stopping keeps no places behind.
                        if !on {
                            progress::forget_all(cx);
                        }
                    }),
                ),
                row(
                    "Show chapters on the seek bar",
                    Some("Where each part of a file with chapters starts".into()),
                    theme,
                    Switch::new("chapter-marks", s.chapter_marks).on_toggle(|&on, _, cx| {
                        update_settings(cx, true, |s| s.chapter_marks = on)
                    }),
                ),
                row(
                    "Volume for new files",
                    None,
                    theme,
                    percent_slider(view, "default-volume", s.default_volume, theme, |s, v| {
                        s.default_volume = v
                    }),
                ),
                row(
                    "Seek step",
                    Some("How far the seek shortcuts move, ← and → at first".into()),
                    theme,
                    Segmented::new(
                        "seek-step",
                        &[(5, "5 s"), (10, "10 s"), (30, "30 s"), (60, "60 s")],
                        s.seek_step_seconds,
                    )
                    .on_select(|&v, _, cx| update_settings(cx, true, |s| s.seek_step_seconds = v)),
                ),
            ],
        )]
    }
}
