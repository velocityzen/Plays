//! The mouse in and out of the window, and resting in it, and the traffic
//! lights, which fade out with the controls while it is away or resting.

use gpui::prelude::*;
use gpui::{
    Context, DispatchPhase, MouseDownEvent, MouseExitEvent, MouseMoveEvent, ScrollWheelEvent,
    WeakEntity, Window, canvas,
};

use super::Workspace;
use crate::settings::{ControlsVisibility, Settings};
use crate::ui::theme::motion::MOUSE_IDLE;
use crate::ui::traffic_lights;

impl Workspace {
    fn mouse_moved(&mut self, cx: &mut Context<Self>) {
        self.used(cx);
        self.mouse_moved_at = cx.background_executor().now();
        if !self.mouse_inside || self.mouse_idle || self.keyboard_focus {
            self.mouse_inside = true;
            self.mouse_idle = false;
            // The block under it is in front now.
            self.keyboard_focus = false;
            self.blocks_changed(cx);
        }
        if !self.watching_idle {
            self.watching_idle = true;
            self.watch_idle(cx);
        }
    }

    fn mouse_left(&mut self, cx: &mut Context<Self>) {
        if self.mouse_inside {
            self.mouse_inside = false;
            self.blocks_changed(cx);
        }
    }

    /// Once the mouse has rested for `MOUSE_IDLE`, take it as gone from
    /// playing blocks: their controls fade, and the pointer hides over them.
    fn watch_idle(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut wait = MOUSE_IDLE;
            loop {
                cx.background_executor().timer(wait).await;
                let rested = this.update(cx, |this, cx| {
                    let rested = cx
                        .background_executor()
                        .now()
                        .saturating_duration_since(this.mouse_moved_at);
                    if rested < MOUSE_IDLE {
                        return Some(MOUSE_IDLE - rested);
                    }
                    this.watching_idle = false;
                    if this.mouse_inside {
                        this.mouse_idle = true;
                        this.blocks_changed(cx);
                    }
                    None
                });
                match rested {
                    Ok(Some(left)) => wait = left,
                    _ => break,
                }
            }
        })
        .detach();
    }

    /// Like the controls, the traffic lights fade out while something plays
    /// and the mouse is away from the window or resting.
    pub(super) fn update_traffic_lights(&mut self, window: &Window, cx: &Context<Self>) {
        // The screensaver hides them too.
        let show = !self.screensaver.bouncing()
            && ((self.mouse_inside
                && (!self.mouse_idle || self.blocks.iter().any(|t| t.view.read(cx).over_bars())))
                || cx.global::<Settings>().controls == ControlsVisibility::Always
                || !self.any_playing(cx));
        if show != self.traffic_lights_shown {
            self.traffic_lights_shown = show;
            traffic_lights::set_visible(window, show);
        }
    }
}

/// Follows the mouse in and out of the window. gpui keeps the last position
/// after the mouse leaves, so hovering alone would leave things showing.
/// A press or scroll, wherever it goes, uses the window too.
pub(super) fn track_mouse(workspace: WeakEntity<Workspace>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            window.on_mouse_event({
                let workspace = workspace.clone();
                move |_: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        workspace.update(cx, |this, cx| this.used(cx)).ok();
                    }
                }
            });
            window.on_mouse_event({
                let workspace = workspace.clone();
                move |_: &ScrollWheelEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        workspace.update(cx, |this, cx| this.used(cx)).ok();
                    }
                }
            });
            window.on_mouse_event({
                let workspace = workspace.clone();
                move |_: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        workspace.update(cx, |this, cx| this.mouse_moved(cx)).ok();
                    }
                }
            });
            window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    workspace.update(cx, |this, cx| this.mouse_left(cx)).ok();
                }
            });
        },
    )
    .absolute()
    .size_0()
}
