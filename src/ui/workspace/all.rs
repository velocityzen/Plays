//! What the "all" commands do: to every block in every player window, so a
//! block popped out onto another display stays with the others, whether it
//! plays here or on a device it casts to.

use gpui::{App, Entity, WindowId};

use super::Workspace;
use crate::app::player_windows;
use crate::settings::Settings;
use crate::ui::player::PlayerView;

impl Workspace {
    /// Whether a block in this window plays.
    pub(super) fn any_playing(&self, cx: &App) -> bool {
        self.blocks
            .iter()
            .any(|block| block.view.read(cx).is_playing())
    }

    /// Whether it ticks often: something in it plays, or it's on its way
    /// to another display.
    pub(super) fn busy(&self, cx: &App) -> bool {
        self.any_playing(cx) || self.moving_to.is_some()
    }
}

/// The blocks in every player window but `except`, window by window as
/// ⌘1 to ⌘9 count them.
fn views_but(except: Option<WindowId>, cx: &App) -> Vec<Entity<PlayerView>> {
    let mut views = Vec::new();
    for window in player_windows(cx) {
        if Some(window.window_id()) == except {
            continue;
        }
        if let Some(Ok(workspace)) = window.downcast::<Workspace>().map(|window| window.read(cx)) {
            views.extend(workspace.blocks.iter().map(|block| block.view.clone()));
        }
    }
    views
}

/// The blocks in every player window.
pub fn views(cx: &App) -> Vec<Entity<PlayerView>> {
    views_but(None, cx)
}

/// Whether a block plays in any player window.
pub fn any_playing(cx: &App) -> bool {
    views(cx).iter().any(|view| view.read(cx).is_playing())
}

/// Whether a block plays in a player window other than `window`: as that
/// window draws, its own can't be read through it.
pub fn playing_elsewhere(window: WindowId, cx: &App) -> bool {
    views_but(Some(window), cx)
        .iter()
        .any(|view| view.read(cx).is_playing())
}

fn for_each(cx: &mut App, f: impl Fn(&mut PlayerView)) {
    for view in views(cx) {
        view.update(cx, |view, cx| {
            f(view);
            cx.notify();
        });
    }
}

pub fn play(cx: &mut App) {
    for_each(cx, PlayerView::play);
}

pub fn pause(cx: &mut App) {
    for_each(cx, PlayerView::pause);
}

/// Pause every block if any plays, else play them all.
pub fn toggle(cx: &mut App) {
    if any_playing(cx) {
        pause(cx);
    } else {
        play(cx);
    }
}

pub fn restart(cx: &mut App) {
    for_each(cx, |view| view.seek(0.0));
}

/// Skip every block ahead by the seek step (`1`), or back (`-1`).
pub fn seek(direction: f64, cx: &mut App) {
    let step = cx.global::<Settings>().seek_step_seconds as f64 * direction;
    for view in views(cx) {
        view.update(cx, |view, cx| view.seek_by(step, cx));
    }
}

/// Pause every block and show its next frame (`1`) or the one before
/// (`-1`), each by its own frame rate.
pub fn step_frame(direction: f64, cx: &mut App) {
    for view in views(cx) {
        view.update(cx, |view, cx| view.step_frame(direction, cx));
    }
}

/// Every block to where `to` is, playing or paused as it is. It seeks
/// there too, so they all go on from the same time.
pub fn sync_to(to: &Entity<PlayerView>, cx: &mut App) {
    let (time, playing) = {
        let to = to.read(cx);
        (to.position(), to.is_playing())
    };
    for_each(cx, |view| {
        view.seek(time);
        if playing {
            view.play();
        } else {
            view.pause();
        }
    });
}

/// Every block's volume up (or down, `delta` below 0), each from its own.
pub fn change_volume(delta: f32, cx: &mut App) {
    for view in views(cx) {
        view.update(cx, |view, cx| view.change_volume(delta, cx));
    }
}

/// Mute every block if any plays its sound, else unmute them all.
pub fn toggle_mute(cx: &mut App) {
    let mute = views(cx)
        .iter()
        .any(|view| !view.read(cx).player().is_muted());
    for_each(cx, |view| view.set_muted(mute));
}
