use gpui::prelude::*;
use gpui::{App, ClickEvent, ElementId, Hsla, SharedString, Window, div};

use super::{ClickHandler, Icon};
use crate::ui::theme::{Theme, controls, overlay, radius};

/// A square, borderless button showing one icon.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: SharedString,
    color: Option<Hsla>,
    overlay: bool,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon: icon.into(),
            color: None,
            overlay: false,
            on_click: None,
        }
    }

    /// The icon's color; defaults to the text color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Colors for drawing over video, which is dark in both themes.
    pub fn overlay(mut self) -> Self {
        self.overlay = true;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let (color, hover) = if self.overlay {
            (overlay::text(), overlay::hover())
        } else {
            (theme.text, theme.surface_hover)
        };
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(controls::ICON_BUTTON)
            .superellipse(radius::CONTROL)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .when_some(self.on_click, |this, handler| this.on_click(handler))
            .child(Icon::new(self.icon).color(self.color.unwrap_or(color)))
    }
}
