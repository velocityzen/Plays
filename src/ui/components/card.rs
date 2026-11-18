use gpui::prelude::*;
use gpui::{AnyElement, App, Refineable, StyleRefinement, Window, div};

use super::{ComponentSize, Sizable};
use crate::ui::theme::{Theme, radius};

/// A flat card: the surface color, a hairline border and rounded corners,
/// as for a group of settings. Its size sets its corners. Lay it out and
/// fill it like a `div`; styles set on it win over its own.
#[derive(IntoElement, Default)]
pub struct Card {
    style: StyleRefinement,
    children: Vec<AnyElement>,
    size: ComponentSize,
}

impl Card {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Sizable for Card {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for Card {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let mut card = div()
            .superellipse(self.size.pick(radius::CARD))
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface);
        card.style().refine(&self.style);
        card.children(self.children)
    }
}
