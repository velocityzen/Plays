use gpui::prelude::*;
use gpui::{AnyElement, App, Refineable, StyleRefinement, Window, div, px};

use crate::ui::theme::{Theme, overlay};

/// A floating panel to put anything in. By default it is a menu's: the
/// theme's surface color with a hairline border, rounded corners and a
/// shadow. [`overlay`](Self::overlay) makes it a note's on the picture
/// instead. Text in it is small. Size, lay out and fill it like a `div`;
/// styles set on it win over its own.
#[derive(IntoElement, Default)]
pub struct Panel {
    style: StyleRefinement,
    children: Vec<AnyElement>,
    occlude: bool,
    overlay: bool,
}

impl Panel {
    pub fn new() -> Self {
        Self::default()
    }

    /// For drawing over video, which is dark in both themes: see-through
    /// black with light text and well-rounded superellipse corners, and no
    /// border or shadow. Use `overlay::text_muted()` for dimmer text in it.
    pub fn overlay(mut self) -> Self {
        self.overlay = true;
        self
    }

    /// Take the mouse, so clicks and hovers don't reach what is under it:
    /// for menus, but not for notes, which would stop the tile under them
    /// counting as hovered.
    pub fn occlude(mut self) -> Self {
        self.occlude = true;
        self
    }
}

impl Styled for Panel {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Panel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Panel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let panel = div().overflow_hidden().text_sm();
        let mut panel = if self.overlay {
            panel
                .superellipse(px(24.))
                .bg(overlay::scrim())
                .text_color(overlay::text())
        } else {
            let theme = Theme::get(window, cx);
            panel
                .rounded(px(8.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .shadow_lg()
                .text_color(theme.text)
        };
        panel.style().refine(&self.style);
        panel
            .when(self.occlude, |this| this.occlude())
            .children(self.children)
    }
}
