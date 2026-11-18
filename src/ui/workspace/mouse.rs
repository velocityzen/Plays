//! The mouse in and out of the window, and the traffic lights, which fade
//! out with the controls while it is away.

use gpui::prelude::*;
use gpui::{Context, DispatchPhase, MouseExitEvent, MouseMoveEvent, WeakEntity, Window, canvas};

use super::Workspace;
use crate::settings::{ControlsVisibility, Settings};
use crate::ui::traffic_lights;

impl Workspace {
    fn set_mouse_inside(&mut self, inside: bool, cx: &mut Context<Self>) {
        if self.mouse_inside != inside {
            self.mouse_inside = inside;
            self.tiles_changed(cx);
        }
    }

    /// Like the controls, the traffic lights fade out while something plays
    /// and the mouse is away from the window.
    pub(super) fn update_traffic_lights(&mut self, window: &Window, cx: &Context<Self>) {
        let show = self.mouse_inside
            || cx.global::<Settings>().controls == ControlsVisibility::Always
            || !self.any_playing(cx);
        if show != self.traffic_lights_shown {
            self.traffic_lights_shown = show;
            traffic_lights::set_visible(window, show);
        }
    }
}

/// Follows the mouse in and out of the window. gpui keeps the last position
/// after the mouse leaves, so hovering alone would leave things showing.
pub(super) fn track_mouse(workspace: WeakEntity<Workspace>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            window.on_mouse_event({
                let workspace = workspace.clone();
                move |_: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        workspace
                            .update(cx, |this, cx| this.set_mouse_inside(true, cx))
                            .ok();
                    }
                }
            });
            window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    workspace
                        .update(cx, |this, cx| this.set_mouse_inside(false, cx))
                        .ok();
                }
            });
        },
    )
    .absolute()
    .size_0()
}
