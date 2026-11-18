use gpui::prelude::*;
use gpui::{App, Hsla, Pixels, SharedString, Window, svg};

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
            size: controls::ICON,
            color: None,
        }
    }

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
