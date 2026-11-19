use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, Refineable, SharedString, StyleRefinement, Window, div,
    white,
};

use super::{ClickHandler, ComponentSize, Sizable};
use crate::ui::theme::menu::{CHECK_WIDTH, DETAIL_HOVER, ROW_HEIGHT, ROW_PADDING};
use crate::ui::theme::{Theme, overlay, radius};

/// Rows light up as a whole, details included.
const GROUP: &str = "menu-row";

/// A row in a menu: a check if it is the one picked, its name, cut short
/// if need be, and a detail after it, dimmer. One that can be clicked
/// lights up in the accent color while hovered. Place it like a `div`;
/// styles set on it win over its own.
#[derive(IntoElement)]
pub struct MenuRow {
    id: ElementId,
    label: SharedString,
    checked: bool,
    detail: Option<AnyElement>,
    overlay: bool,
    size: ComponentSize,
    on_click: Option<ClickHandler>,
    style: StyleRefinement,
}

impl MenuRow {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            checked: false,
            detail: None,
            overlay: false,
            size: ComponentSize::Medium,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    /// Shows the check: it is the one picked.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// After the name, in the dimmer color, such as a track's codec or a
    /// font's sample.
    pub fn detail(mut self, detail: impl IntoElement) -> Self {
        self.detail = Some(detail.into_any_element());
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

impl Sizable for MenuRow {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for MenuRow {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MenuRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let muted = if self.overlay {
            overlay::text_muted()
        } else {
            theme.text_muted
        };
        let accent = theme.accent;
        let size = self.size;
        let clickable = self.on_click.is_some();
        // Tests find a row by its id.
        let selector = self.id.clone();
        let mut row = size
            .text(div())
            .id(self.id)
            .debug_selector(move || selector.to_string())
            .flex()
            .items_center()
            .gap_1p5()
            .h(size.pick(ROW_HEIGHT))
            .px(size.pick(ROW_PADDING))
            .superellipse(size.pick(radius::ITEM))
            // A character rather than an icon, so it turns white with the
            // rest of the row.
            .child(
                div()
                    .flex_none()
                    .w(size.pick(CHECK_WIDTH))
                    .when(self.checked, |this| this.child("✓")),
            )
            .child(div().flex_1().min_w_0().truncate().child(self.label))
            .when_some(self.detail, |this, detail| {
                this.child(
                    div()
                        .flex_none()
                        .text_color(muted)
                        .when(clickable, |this| {
                            this.group_hover(GROUP, |style| {
                                style.text_color(white().opacity(DETAIL_HOVER))
                            })
                        })
                        .child(detail),
                )
            })
            .when_some(self.on_click, |this, handler| {
                this.group(GROUP)
                    .cursor_pointer()
                    .hover(move |style| style.bg(accent).text_color(white()))
                    .on_click(handler)
            });
        row.style().refine(&self.style);
        row
    }
}
