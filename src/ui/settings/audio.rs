//! Audio: the volume of everything, and the output it plays through.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Window, div};

use super::form::{group, percent_slider, row};
use super::{Pane, SettingsView};
use crate::app::AudioEngine;
use crate::settings::Settings;
use crate::ui::theme::Theme;
use crate::ui::theme::settings::{AUDIO_COLOR, OUTPUT_MAX_WIDTH};

pub(super) struct AudioPane;

impl Pane for AudioPane {
    fn name(&self) -> &'static str {
        "Audio"
    }

    fn icon(&self) -> &'static str {
        "volume"
    }

    fn color(&self) -> u32 {
        AUDIO_COLOR
    }

    fn content(
        &self,
        view: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        vec![group(
            None,
            theme,
            vec![
                row(
                    "Master volume",
                    None,
                    theme,
                    percent_slider(view, "master-volume", s.master_volume, theme, |s, v| {
                        s.master_volume = v
                    }),
                ),
                row(
                    "Output",
                    None,
                    theme,
                    div()
                        .text_sm()
                        .text_color(theme.text_muted)
                        .max_w(OUTPUT_MAX_WIDTH)
                        .truncate()
                        .child(AudioEngine::device_description(cx)),
                ),
            ],
        )]
    }
}
