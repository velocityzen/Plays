//! Reusable components, one per file. Each is a [`RenderOnce`](gpui::RenderOnce)
//! component set up with builder methods, which picks its colors from the
//! theme when drawn. Every one comes in three sizes ([`Sizable`]): small,
//! medium (the default) and large, with its numbers for each in `theme`.

#![allow(
    dead_code,
    reason = "a kit: some sizes, variants and states have no use in the app yet"
)]

mod badge;
mod button;
mod card;
mod icon;
mod menu_row;
mod meter;
mod panel;
mod popover;
mod segmented;
mod slider;
mod switch;

pub use badge::Badge;
#[allow(unused_imports, reason = "for picking a variant or side at run time")]
pub use button::{Button, ButtonVariant, IconSide};
pub use card::Card;
pub use icon::Icon;
pub use menu_row::MenuRow;
pub use meter::Meter;
pub use panel::Panel;
pub use popover::Popover;
pub use segmented::Segmented;
pub use slider::{Slide, Slider, SliderEvent};
pub use switch::Switch;

use gpui::{App, ClickEvent, Styled, Window};

use crate::ui::theme::{controls, radius};

/// What a button runs when clicked.
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The three sizes every component comes in.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum ComponentSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl ComponentSize {
    /// This size's value out of `[small, medium, large]`, as `theme` lists
    /// them.
    pub fn pick<T: Copy>(self, values: [T; 3]) -> T {
        values[self as usize]
    }

    /// This size's text. Large is as small as medium, as on macOS, only
    /// with more room around it.
    pub fn text<E: Styled>(self, element: E) -> E {
        match self {
            Self::Small => element.text_xs(),
            Self::Medium | Self::Large => element.text_sm(),
        }
    }
}

/// A component that comes in [`ComponentSize`]s.
pub trait Sizable: Sized {
    fn with_size(self, size: ComponentSize) -> Self;

    fn small(self) -> Self {
        self.with_size(ComponentSize::Small)
    }

    fn large(self) -> Self {
        self.with_size(ComponentSize::Large)
    }
}

/// The frame buttons, text fields and hotkey chips share: their height,
/// the room on their sides, a hairline border, and corners and text for
/// their size.
pub trait ControlFrame: Styled + Sized {
    fn control_frame(self, size: ComponentSize) -> Self {
        size.text(
            self.flex()
                .items_center()
                .h(size.pick(controls::HEIGHT))
                .px(size.pick(controls::PADDING))
                .superellipse(size.pick(radius::CONTROL))
                .border_1(),
        )
    }
}

impl<T: Styled> ControlFrame for T {}
