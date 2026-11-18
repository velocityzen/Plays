//! Reusable controls, one per file. Each is a [`RenderOnce`](gpui::RenderOnce)
//! component set up with builder methods, which picks its colors from the
//! theme when drawn.

mod button;
mod icon;
mod icon_button;
mod panel;
mod segmented;
mod slider;
mod switch;

pub use button::Button;
pub use icon::Icon;
pub use icon_button::IconButton;
pub use panel::Panel;
pub use segmented::Segmented;
pub use slider::{Slide, Slider, SliderEvent};
pub use switch::Switch;

use gpui::{App, ClickEvent, Window};

/// What a button runs when clicked.
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
