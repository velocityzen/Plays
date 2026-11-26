//! Playback: what files do when they open, several at once too, and how
//! far seeking goes.

use gpui::{AnyElement, Context, Window};

use super::form::{group, percent_slider, row, set};
use super::{Pane, SettingsView};
use crate::progress::{self, KEEP_DAYS};
use crate::settings::{Settings, fields};
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
                        .on_toggle(|&on, _, cx| set("autoplay", on, cx)),
                ),
                row(
                    "Play several files",
                    Some(
                        "Opened or dropped at once, or a folder's: a block each, all \
                         playing, or one block playing them in turn"
                            .into(),
                    ),
                    theme,
                    Segmented::new("several-files", &fields::SEVERAL_FILES, s.several_files)
                        .on_select(|&v, _, cx| set("several_files", v, cx)),
                ),
                row(
                    "Loop new files",
                    None,
                    theme,
                    Switch::new("loop", s.loop_by_default)
                        .on_toggle(|&on, _, cx| set("loop_by_default", on, cx)),
                ),
                row(
                    "Resume files where they were left",
                    Some(
                        format!(
                            "Each opens where it was and as it was set, for {KEEP_DAYS} days since it last played"
                        )
                        .into(),
                    ),
                    theme,
                    Switch::new("resume", s.resume_files).on_toggle(|&on, _, cx| {
                        set("resume_files", on, cx);
                        // Stopping keeps no places behind.
                        if !on {
                            progress::forget_all(cx);
                        }
                    }),
                ),
                row(
                    "Show chapters on the seek bar",
                    Some("Where each chapter starts, and its name under the mouse".into()),
                    theme,
                    Switch::new("chapter-marks", s.chapter_marks)
                        .on_toggle(|&on, _, cx| set("chapter_marks", on, cx)),
                ),
                row(
                    "Volume for new files",
                    None,
                    theme,
                    percent_slider(
                        view,
                        "default-volume",
                        "default_volume",
                        s.default_volume(),
                        theme,
                    ),
                ),
                row(
                    "Seek step",
                    Some("How far the seek shortcuts move, ← and → at first".into()),
                    theme,
                    Segmented::new("seek-step", &fields::SEEK_STEPS, s.seek_step_seconds)
                        .on_select(|&v, _, cx| set("seek_step_seconds", v, cx)),
                ),
            ],
        )]
    }
}
