//! Audio Mode: how an audio file's block looks, what plays behind its
//! cover and whether its quality shows.

use gpui::{AnyElement, Context, Window};

use super::form::{group, row, set};
use super::{Pane, SettingsView};
use crate::settings::{Settings, fields};
use crate::ui::components::{Segmented, Switch};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::AUDIO_MODE_COLOR;

pub(super) struct AudioModePane;

impl Pane for AudioModePane {
    fn name(&self) -> &'static str {
        "Audio Mode"
    }

    fn icon(&self) -> &'static str {
        "music"
    }

    fn color(&self) -> u32 {
        AUDIO_MODE_COLOR
    }

    fn content(
        &self,
        _: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        _: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let mode = s.audio_mode;
        vec![group(
            None,
            theme,
            vec![
                row(
                    "Visualizer",
                    Some("What plays behind an audio file's cover".into()),
                    theme,
                    Segmented::new("visualizer", &fields::VISUALIZERS, mode.visualizer)
                        .on_select(|&v, _, cx| set("audio_mode.visualizer", v, cx)),
                ),
                row(
                    "Show quality",
                    Some(
                        "Its codec, bits, sample rate, channels and bit rate, under its tags"
                            .into(),
                    ),
                    theme,
                    Switch::new("show-quality", mode.show_quality)
                        .on_toggle(|&on, _, cx| set("audio_mode.show_quality", on, cx)),
                ),
            ],
        )]
    }
}
