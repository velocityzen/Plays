use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, ElementId, Window, div, px};

use crate::ui::theme::Theme;

type SelectHandler<T> = Rc<dyn Fn(&T, &mut Window, &mut App) + 'static>;

/// A row of mutually exclusive choices.
#[derive(IntoElement)]
pub struct Segmented<T: Copy + PartialEq + 'static> {
    id: ElementId,
    options: Vec<(T, &'static str)>,
    selected: T,
    on_select: Option<SelectHandler<T>>,
}

impl<T: Copy + PartialEq + 'static> Segmented<T> {
    /// `options` pairs each value with its label.
    pub fn new(id: impl Into<ElementId>, options: &[(T, &'static str)], selected: T) -> Self {
        Self {
            id: id.into(),
            options: options.to_vec(),
            selected,
            on_select: None,
        }
    }

    /// Called with the value of the choice that was clicked.
    pub fn on_select(mut self, handler: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl<T: Copy + PartialEq + 'static> RenderOnce for Segmented<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let (text, muted, surface) = (theme.text, theme.text_muted, theme.surface);
        let selected = self.selected;
        let on_select = self.on_select;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .flex_wrap()
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(7.))
            .bg(theme.surface_active)
            .children(
                self.options
                    .into_iter()
                    .enumerate()
                    .map(|(index, (value, label))| {
                        let active = value == selected;
                        div()
                            .id(index)
                            .flex()
                            .items_center()
                            .h(px(24.))
                            .px_2p5()
                            .rounded(px(5.))
                            .text_sm()
                            .cursor_pointer()
                            .when(active, |this| this.bg(surface).text_color(text).shadow_sm())
                            .when(!active, |this| {
                                this.text_color(muted)
                                    .hover(move |style| style.text_color(text))
                            })
                            .when_some(on_select.clone(), |this, handler| {
                                this.on_click(move |_, window, cx| handler(&value, window, cx))
                            })
                            .child(label)
                    }),
            )
    }
}
