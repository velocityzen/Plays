//! What the Playback commands do to every tile in the window.

use gpui::Context;

use super::Workspace;
use crate::media::Player;
use crate::settings::Settings;

impl Workspace {
    fn for_each_player(&mut self, cx: &mut Context<Self>, f: impl Fn(&mut Player)) {
        for tile in &self.tiles {
            tile.view.update(cx, |view, cx| {
                f(view.player_mut());
                cx.notify();
            });
        }
    }

    pub(super) fn any_playing(&self, cx: &Context<Self>) -> bool {
        self.tiles
            .iter()
            .any(|t| t.view.read(cx).player().is_playing())
    }

    pub fn play_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_player(cx, |p| p.play());
    }

    pub fn pause_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_player(cx, |p| p.pause());
    }

    pub fn toggle_all(&mut self, cx: &mut Context<Self>) {
        if self.any_playing(cx) {
            self.pause_all(cx);
        } else {
            self.play_all(cx);
        }
    }

    pub fn restart_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_player(cx, |p| p.seek(0.0));
    }

    pub fn seek_all(&mut self, direction: f64, cx: &mut Context<Self>) {
        let step = cx.global::<Settings>().seek_step_seconds as f64 * direction;
        self.for_each_player(cx, |p| p.seek_by(step));
    }

    pub fn toggle_mute_all(&mut self, cx: &mut Context<Self>) {
        let mute = self
            .tiles
            .iter()
            .any(|t| !t.view.read(cx).player().is_muted());
        self.for_each_player(cx, |p| p.set_muted(mute));
    }
}
