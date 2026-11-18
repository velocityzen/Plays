use gpui::prelude::*;
use gpui::{App, Hsla, Pixels, SharedString, Window, svg};

use super::{ComponentSize, Sizable};
use crate::ui::theme::{Theme, controls};

/// One of the app's icons (`assets/icons`), drawn in a single color.
#[derive(IntoElement)]
pub struct Icon {
    name: SharedString,
    size: Pixels,
    color: Option<Hsla>,
}

impl Icon {
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            size: ComponentSize::Medium.pick(controls::ICON),
            color: None,
        }
    }

    /// A size other than the three of [`Sizable`].
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Defaults to the theme's text color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl Sizable for Icon {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size.pick(controls::ICON);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or_else(|| Theme::get(window, cx).text);
        svg()
            .path(SharedString::from(format!("icons/{}.svg", self.name)))
            .flex_none()
            .size(self.size)
            .text_color(color)
    }
}
