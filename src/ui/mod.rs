pub mod components;
pub mod open_url;
pub mod player_view;
pub mod settings_view;
mod system_colors;
pub mod text_input;
pub mod theme;
mod traffic_lights;
pub mod workspace;

use gpui::{Pixels, Point, point, px};

/// Where the macOS traffic lights go in windows without a title bar.
pub const TRAFFIC_LIGHTS: Point<Pixels> = point(px(14.), px(14.));

/// The room a bar under the traffic lights leaves on its left.
pub const TRAFFIC_LIGHTS_WIDTH: Pixels = px(80.);
