use gpui::prelude::*;
use gpui::{App, ElementId, Window, div, white};

use super::{ComponentSize, Sizable};
use crate::ui::theme::Theme;
use crate::ui::theme::controls::{DISABLED, SWITCH, SWITCH_PADDING};

type ToggleHandler = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// An on/off switch.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    size: ComponentSize,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            size: ComponentSize::Medium,
            disabled: false,
            on_toggle: None,
        }
    }

    /// Dimmed, and clicks do nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the new state when the switch is clicked.
    pub fn on_toggle(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl Sizable for Switch {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let on = self.on;
        let size = self.size.pick(SWITCH);
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .when(on, |this| this.justify_end())
            .w(size.width)
            .h(size.height)
            .p(SWITCH_PADDING)
            .rounded_full()
            .bg(if on { theme.accent } else { theme.track })
            .map(|this| {
                if self.disabled {
                    return this.opacity(DISABLED);
                }
                this.cursor_pointer()
                    .when_some(self.on_toggle, |this, handler| {
                        this.on_click(move |_, window, cx| handler(&!on, window, cx))
                    })
            })
            .child(
                div()
                    .size(size.height - SWITCH_PADDING * 2.)
                    .rounded_full()
                    .bg(white()),
            )
    }
}
