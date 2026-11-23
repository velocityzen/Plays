//! What plays behind a sound file's cover and lines: a visualizer, drawn by
//! a shader of its own, fed what's playing.

use gpui::prelude::*;
use gpui::{Hsla, VisualizerShader, canvas};

/// A speaker filling what it's in, in `color`: two thin rings, blurred a
/// little, its rim and its cone's edge, which move out with `level`, from
/// 0 to 1.
pub(super) fn speaker(level: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            window.paint_visualizer(bounds, VisualizerShader::Speaker { level }, color);
        },
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
#[path = "tests/visualizer.rs"]
mod tests;
