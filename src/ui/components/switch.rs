use gpui::prelude::*;
use gpui::{App, ElementId, Window, div, px, white};

use crate::ui::theme::Theme;

type ToggleHandler = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// An on/off switch.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            on_toggle: None,
        }
    }

    /// Called with the new state when the switch is clicked.
    pub fn on_toggle(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let on = self.on;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .when(on, |this| this.justify_end())
            .w(px(36.))
            .h(px(20.))
            .p(px(2.))
            .rounded_full()
            .cursor_pointer()
            .bg(if on { theme.accent } else { theme.track })
            .when_some(self.on_toggle, |this, handler| {
                this.on_click(move |_, window, cx| handler(&!on, window, cx))
            })
            .child(div().size(px(16.)).rounded_full().bg(white()))
    }
}
