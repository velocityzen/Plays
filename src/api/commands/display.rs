//! `display`: the displays connected, which windows show on.

use gpui::App;
use serde_json::{Value, json};

use crate::api::registry::{Scope, Spec};
use crate::ui::displays;

pub const COMMANDS: &[Spec] = &[Spec::query(
    "display.ls",
    "List Displays",
    "List the displays connected, in the system's order.",
    Scope::App,
    "[{id, name, bounds: {x, y, width, height}}]: each display's id, as \
     window.show_on_display and window.inspect have it, its name, and where it is, in \
     points.",
    |_, _, cx| Ok(listing(cx)),
)
.keywords(&["list", "ls", "screens", "monitors"])];

/// What `display.ls` says of each display.
fn listing(cx: &App) -> Value {
    displays::connected(cx)
        .iter()
        .map(|display| {
            let bounds = display.bounds;
            json!({
                "id": display.id,
                "name": display.name,
                "bounds": {
                    "x": f32::from(bounds.origin.x),
                    "y": f32::from(bounds.origin.y),
                    "width": f32::from(bounds.size.width),
                    "height": f32::from(bounds.size.height),
                },
            })
        })
        .collect()
}
