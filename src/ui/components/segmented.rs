use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, ElementId, Window, div};

use super::{ComponentSize, Sizable};
use crate::ui::theme::controls::{DISABLED, HEIGHT, PADDING, SEGMENTED_PADDING};
use crate::ui::theme::{Theme, radius};

type SelectHandler<T> = Rc<dyn Fn(&T, &mut Window, &mut App) + 'static>;

/// A row of mutually exclusive choices, as high as a button of its size.
#[derive(IntoElement)]
pub struct Segmented<T: Copy + PartialEq + 'static> {
    id: ElementId,
    options: Vec<(T, &'static str)>,
    selected: T,
    size: ComponentSize,
    disabled: bool,
    on_select: Option<SelectHandler<T>>,
}

impl<T: Copy + PartialEq + 'static> Segmented<T> {
    /// `options` pairs each value with its label.
    pub fn new(id: impl Into<ElementId>, options: &[(T, &'static str)], selected: T) -> Self {
        Self {
            id: id.into(),
            options: options.to_vec(),
            selected,
            size: ComponentSize::Medium,
            disabled: false,
            on_select: None,
        }
    }

    /// Dimmed, and clicks do nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the value of the choice that was clicked.
    pub fn on_select(mut self, handler: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl<T: Copy + PartialEq + 'static> Sizable for Segmented<T> {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl<T: Copy + PartialEq + 'static> RenderOnce for Segmented<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let (text, muted, surface) = (theme.text, theme.text_muted, theme.surface);
        let selected = self.selected;
        let size = self.size;
        let enabled = !self.disabled;
        let on_select = self.on_select.filter(|_| enabled);
        let corner = size.pick(radius::ITEM);
        // Tests find a choice as the row's id and its label: "panels-Glass".
        let group = self.id.to_string();
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .flex_wrap()
            .p(SEGMENTED_PADDING)
            .gap(SEGMENTED_PADDING)
            // Round about the segments' corners, the padding away.
            .superellipse(corner + SEGMENTED_PADDING)
            .bg(theme.surface_active)
            .when(!enabled, |this| this.opacity(DISABLED))
            .children(
                self.options
                    .into_iter()
                    .enumerate()
                    .map(|(index, (value, label))| {
                        let active = value == selected;
                        let group = group.clone();
                        size.text(div())
                            .id(index)
                            .debug_selector(move || format!("{group}-{label}"))
                            .flex()
                            .items_center()
                            .h(size.pick(HEIGHT) - SEGMENTED_PADDING * 2.)
                            .px(size.pick(PADDING))
                            .superellipse(corner)
                            .when(enabled, |this| this.cursor_pointer())
                            .when(active, |this| this.bg(surface).text_color(text).shadow_sm())
                            .when(!active, |this| {
                                this.text_color(muted).when(enabled, |this| {
                                    this.hover(move |style| style.text_color(text))
                                })
                            })
                            .when_some(on_select.clone(), |this, handler| {
                                this.on_click(move |_, window, cx| handler(&value, window, cx))
                            })
                            .child(label)
                    }),
            )
    }
}
