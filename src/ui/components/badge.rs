use gpui::prelude::*;
use gpui::{App, FontWeight, Hsla, Refineable, SharedString, StyleRefinement, Window, div, white};

use super::{ComponentSize, Icon, Sizable};
use crate::ui::theme::controls::{BADGE, BADGE_ICON, BADGE_PADDING};
use crate::ui::theme::radius;

enum Content {
    Icon(SharedString),
    Label(SharedString),
}

/// A small tag in a color with white on it: an icon on a square, as for a
/// pane in the Settings sidebar, or a word, such as LIVE. Place it like a
/// `div`; styles set on it win over its own.
#[derive(IntoElement)]
pub struct Badge {
    content: Content,
    color: Hsla,
    size: ComponentSize,
    style: StyleRefinement,
}

impl Badge {
    pub fn icon(name: impl Into<SharedString>, color: impl Into<Hsla>) -> Self {
        Self::build(Content::Icon(name.into()), color.into())
    }

    pub fn label(text: impl Into<SharedString>, color: impl Into<Hsla>) -> Self {
        Self::build(Content::Label(text.into()), color.into())
    }

    fn build(content: Content, color: Hsla) -> Self {
        Self {
            content,
            color,
            size: ComponentSize::Medium,
            style: StyleRefinement::default(),
        }
    }
}

impl Sizable for Badge {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let size = self.size;
        let badge = div()
            .flex_none()
            .flex()
            .items_center()
            .bg(self.color)
            .text_color(white());
        let mut badge = match self.content {
            Content::Icon(name) => badge
                .justify_center()
                .size(size.pick(BADGE))
                .superellipse(size.pick(radius::ITEM))
                .child(Icon::new(name).size(size.pick(BADGE_ICON)).color(white())),
            // A size smaller than other text, as the word is in capitals.
            Content::Label(text) => badge
                .px(size.pick(BADGE_PADDING))
                .superellipse(size.pick(radius::BADGE))
                .map(|this| match size {
                    ComponentSize::Large => this.text_sm(),
                    _ => this.text_xs(),
                })
                .font_weight(FontWeight::SEMIBOLD)
                .child(text),
        };
        badge.style().refine(&self.style);
        badge
    }
}
