use gpui::prelude::*;
use gpui::{AnyElement, App, Refineable, StyleRefinement, Window, div};

use super::{ComponentSize, Sizable};
use crate::settings::Settings;
use crate::ui::theme::{Theme, backdrop, overlay, panel, radius};

/// A floating panel to put anything in. By default it is a menu's: the
/// theme's surface color with a hairline border, rounded corners and a
/// shadow, and no padding, as a menu's rows keep their own room.
/// [`overlay`](Self::overlay) makes it a panel on the picture instead,
/// padded, and [`note`](Self::note) is such a panel. Its size sets its
/// corners, its text and, over the picture, its padding. Size, lay out and
/// fill it like a `div`; styles set on it win over its own.
#[derive(IntoElement, Default)]
pub struct Panel {
    style: StyleRefinement,
    children: Vec<AnyElement>,
    occlude: bool,
    overlay: bool,
    size: ComponentSize,
}

impl Panel {
    pub fn new() -> Self {
        Self::default()
    }

    /// A note over the picture: a line on a pill.
    pub fn note(text: impl IntoElement) -> Self {
        Self::new()
            .overlay()
            .rounded_full()
            .py(panel::NOTE_PADDING_Y)
            .child(text)
    }

    /// For drawing over video, which is dark in both themes: light text
    /// and well-rounded superellipse corners, no border or shadow, over the
    /// picture as Settings' panel look shows it (see-through black when
    /// plain). Use `overlay::text_muted()` for dimmer text in it.
    pub fn overlay(mut self) -> Self {
        self.overlay = true;
        self
    }

    /// Take the mouse, so clicks and hovers don't reach what is under it:
    /// for menus and dialogs, but not for notes, which would stop the tile
    /// under them counting as hovered.
    pub fn occlude(mut self) -> Self {
        self.occlude = true;
        self
    }
}

impl Sizable for Panel {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
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
        let size = self.size;
        let panel = size.text(div().overflow_hidden());
        let mut panel = if self.overlay {
            let panel = panel
                .superellipse(size.pick(radius::OVERLAY))
                .p(size.pick(panel::OVERLAY_PADDING))
                .text_color(overlay::text());

            match backdrop::for_look(cx.global::<Settings>().panels) {
                Some(backdrop) => panel.backdrop(backdrop),
                None => panel.bg(overlay::scrim()),
            }
        } else {
            let theme = Theme::get(window, cx);
            panel
                .superellipse(size.pick(radius::MENU))
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

/// How each look draws the picture under an overlay panel, drawn off
/// screen by the renderer windows use on macOS (Linux's has the same
/// shaders in WGSL).
#[cfg(all(test, target_os = "macos"))]
#[path = "tests/panel.rs"]
mod tests;
