//! The panels a window's blocks show over their videos: the window's own,
//! which blocks opened in it start out with; each changed for every block
//! in the window as the window's change, and for every window and block as
//! Settings' do.

use gpui::{App, Context};

use super::Workspace;
use crate::settings::{OverVideo, Settings};

impl Workspace {
    /// The panels its blocks show over their videos, as those opened in it
    /// start out.
    pub fn over_video(&self) -> OverVideo {
        self.over_video
    }

    /// Show the panels `shown` has, and hide the others: those that changed
    /// in each of its blocks too.
    pub fn set_over_video(&mut self, shown: OverVideo, cx: &mut Context<Self>) {
        let old = self.over_video;
        self.follow_over_video(old, shown, cx);
    }

    /// Show or hide the panels `old` and `new` tell apart as `new` has
    /// them, in it and each of its blocks.
    fn follow_over_video(&mut self, old: OverVideo, new: OverVideo, cx: &mut Context<Self>) {
        self.over_video = self.over_video.follow(old, new);
        for block in &self.blocks {
            block.view.update(cx, |view, cx| {
                let shown = view.over_video().follow(old, new);
                view.set_over_video(shown, cx);
            });
        }
        cx.notify();
    }
}

/// Show or hide each panel Settings show or hide, as they do, in every
/// window and block.
pub fn follow_settings(cx: &mut App) {
    let mut last = cx.global::<Settings>().over_video;
    cx.observe_global::<Settings>(move |cx| {
        let new = cx.global::<Settings>().over_video;
        let old = std::mem::replace(&mut last, new);
        if old == new {
            return;
        }
        for window in cx.windows() {
            if let Some(window) = window.downcast::<Workspace>() {
                window
                    .update(cx, |workspace, _, cx| {
                        workspace.follow_over_video(old, new, cx);
                    })
                    .ok();
            }
        }
    })
    .detach();
}
