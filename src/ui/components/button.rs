use gpui::prelude::*;
use gpui::{App, ClickEvent, ElementId, SharedString, Window, div, px, white};

use super::{ClickHandler, Icon};
use crate::ui::theme::Theme;

/// A regular text button with an optional leading icon, and optionally its
/// keyboard shortcut after the label.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<SharedString>,
    shortcut: Option<SharedString>,
    primary: bool,
    wide: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            primary: false,
            wide: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, name: impl Into<SharedString>) -> Self {
        self.icon = Some(name.into());
        self
    }

    /// Shown after the label, dimmer, such as "⌘O".
    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// Filled with the accent color, for the main action of a dialog.
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    /// More room on the sides, for a button standing on its own.
    pub fn wide(mut self) -> Self {
        self.wide = true;
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

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let accent = theme.accent;
        let (background, border, hover, active) = if self.primary {
            (accent, accent, accent.opacity(0.85), accent.opacity(0.7))
        } else {
            (
                theme.surface,
                theme.border,
                theme.surface_hover,
                theme.surface_active,
            )
        };
        let (text, icon, shortcut) = if self.primary {
            (white(), white(), white().opacity(0.7))
        } else {
            (theme.text, theme.text_muted, theme.text_muted)
        };
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .h(px(28.))
            .map(|this| {
                if self.wide {
                    this.px_4()
                } else {
                    this.px_2p5()
                }
            })
            .rounded(px(6.))
            .border_1()
            .border_color(border)
            .bg(background)
            .text_sm()
            .text_color(text)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .active(move |style| style.bg(active))
            .when_some(self.on_click, |this, handler| this.on_click(handler))
            .when_some(self.icon, |this, name| {
                this.child(Icon::new(name).size(px(14.)).color(icon))
            })
            .child(self.label)
            .when_some(self.shortcut, |this, keys| {
                this.child(div().ml_0p5().text_color(shortcut).child(keys))
            })
    }
}
