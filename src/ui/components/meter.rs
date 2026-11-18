use gpui::prelude::*;
use gpui::{App, Hsla, Refineable, StyleRefinement, Window, div, relative};

use super::{ComponentSize, Sizable};
use crate::ui::theme::Theme;
use crate::ui::theme::controls::METER_HEIGHT;

/// A rounded bar filled from the left to show a value from 0 to 1, such as
/// a sound's level; a slider's track is one. It is as wide as it can be
/// unless it is given a width; styles set on it win over its own.
#[derive(IntoElement)]
pub struct Meter {
    value: f32,
    track: Option<Hsla>,
    fill: Option<Hsla>,
    size: ComponentSize,
    style: StyleRefinement,
}

impl Meter {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            track: None,
            fill: None,
            size: ComponentSize::Medium,
            style: StyleRefinement::default(),
        }
    }

    /// The color of the bar behind the fill; defaults to the theme's track.
    pub fn track(mut self, color: Hsla) -> Self {
        self.track = Some(color);
        self
    }

    /// The fill's color; defaults to the accent.
    pub fn fill(mut self, color: Hsla) -> Self {
        self.fill = Some(color);
        self
    }
}

impl Sizable for Meter {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for Meter {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Meter {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let mut meter = div()
            .w_full()
            .h(self.size.pick(METER_HEIGHT))
            .rounded_full()
            .bg(self.track.unwrap_or(theme.track))
            .child(
                div()
                    .h_full()
                    .w(relative(self.value.clamp(0.0, 1.0)))
                    .rounded_full()
                    .bg(self.fill.unwrap_or(theme.accent)),
            );
        meter.style().refine(&self.style);
        meter
    }
}
