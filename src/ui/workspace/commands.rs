//! What the Playback commands do to every tile in the window, whether it
//! plays here or on a device it casts to.

use gpui::Context;

use super::Workspace;
use crate::settings::Settings;
use crate::ui::player::PlayerView;

impl Workspace {
    fn for_each_view(&mut self, cx: &mut Context<Self>, f: impl Fn(&mut PlayerView)) {
        for tile in &self.tiles {
            tile.view.update(cx, |view, cx| {
                f(view);
                cx.notify();
            });
        }
    }

    pub(super) fn any_playing(&self, cx: &Context<Self>) -> bool {
        self.tiles.iter().any(|t| t.view.read(cx).is_playing())
    }

    pub fn play_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_view(cx, |view| view.play());
    }

    pub fn pause_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_view(cx, |view| view.pause());
    }

    pub fn toggle_all(&mut self, cx: &mut Context<Self>) {
        if self.any_playing(cx) {
            self.pause_all(cx);
        } else {
            self.play_all(cx);
        }
    }

    pub fn restart_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_view(cx, |view| view.seek(0.0));
    }

    pub fn seek_all(&mut self, direction: f64, cx: &mut Context<Self>) {
        let step = cx.global::<Settings>().seek_step_seconds as f64 * direction;
        for tile in &self.tiles {
            tile.view.update(cx, |view, cx| view.seek_by(step, cx));
        }
    }

    /// Pause every tile and show its next frame (`1`) or the one before
    /// (`-1`), each by its own frame rate.
    pub fn step_frame_all(&mut self, direction: f64, cx: &mut Context<Self>) {
        for tile in &self.tiles {
            tile.view
                .update(cx, |view, cx| view.step_frame(direction, cx));
        }
    }

    /// Every tile to where the tile in front is, playing or paused as it
    /// is. It seeks there too, so they all go on from the same time.
    pub fn sync_all(&mut self, cx: &mut Context<Self>) {
        let Some(front) = self.active_view() else {
            return;
        };
        let (time, playing) = {
            let front = front.read(cx);
            (front.position(), front.is_playing())
        };
        self.for_each_view(cx, |view| {
            view.seek(time);
            if playing {
                view.play();
            } else {
                view.pause();
            }
        });
    }

    /// Every tile's volume up (or down, `delta` below 0), each from its
    /// own.
    pub fn change_volume_all(&mut self, delta: f32, cx: &mut Context<Self>) {
        for tile in &self.tiles {
            tile.view
                .update(cx, |view, cx| view.change_volume(delta, cx));
        }
    }

    pub fn toggle_mute_all(&mut self, cx: &mut Context<Self>) {
        let mute = self
            .tiles
            .iter()
            .any(|t| !t.view.read(cx).player().is_muted());
        self.for_each_view(cx, |view| view.set_muted(mute));
    }
}
