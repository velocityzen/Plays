//! What the commands for many blocks do: to every block in a window, or in
//! every player window, so a block popped out onto another display stays
//! with the others, whether it plays here or on a device it casts to.

use gpui::{App, Context, Entity, WindowId};

use super::Workspace;
use crate::app::player_windows;
use crate::media::tracks::TrackKind;
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

/// Whether a block plays in a player window other than `window`: as that
/// window draws, its own can't be read through it.
pub fn playing_elsewhere(window: WindowId, cx: &App) -> bool {
    views_but(Some(window), cx)
        .iter()
        .any(|view| view.read(cx).is_playing())
}

/// The players of some blocks, to do the same to them all: those in a
/// window, or in every player window.
pub struct Players(Vec<Entity<PlayerView>>);

impl Players {
    /// Those in every player window, window by window as ⌘1 to ⌘9 count
    /// them.
    pub fn everywhere(cx: &App) -> Self {
        Self(views(cx))
    }

    /// Those in `workspace`'s window, in its grid's order.
    pub fn of(workspace: &Workspace) -> Self {
        Self(workspace.players())
    }

    /// Do `f` to each.
    pub fn each(&self, cx: &mut App, mut f: impl FnMut(&mut PlayerView, &mut Context<PlayerView>)) {
        for view in &self.0 {
            view.update(cx, |view, cx| f(view, cx));
        }
    }

    /// Whether `f` holds for each: for none, it does.
    fn all(&self, cx: &App, f: impl Fn(&PlayerView) -> bool) -> bool {
        self.0.iter().all(|view| f(view.read(cx)))
    }

    /// Play each, or pause each (`false`).
    pub fn set_playing(&self, playing: bool, cx: &mut App) {
        self.each(cx, |view, cx| {
            if playing {
                view.play();
            } else {
                view.pause();
            }
            cx.notify();
        });
    }

    /// Pause them all if any plays, else play them all.
    pub fn play_pause(&self, cx: &mut App) {
        let play = self.all(cx, |view| !view.is_playing());
        self.set_playing(play, cx);
    }

    /// Play each that has chapter `index` from its start; whether any has
    /// it.
    pub fn go_to_chapter(&self, index: usize, cx: &mut App) -> bool {
        let mut any = false;
        self.each(cx, |view, cx| {
            if index < view.chapters().len() {
                view.go_to_chapter(index, cx);
                any = true;
            }
        });
        any
    }

    /// Play each one's track `number` of a kind, from 1, or no subtitles
    /// (`None`); whether any has that track.
    pub fn select_track_number(
        &self,
        kind: TrackKind,
        number: Option<usize>,
        cx: &mut App,
    ) -> bool {
        let mut any = false;
        self.each(cx, |view, cx| {
            any |= view.select_track_number(kind, number, cx)
        });
        any
    }

    /// Loop them all, or stop them all looping, as `turn` has it, given
    /// whether all loop.
    pub fn turn_loop(&self, turn: impl FnOnce(bool) -> bool, cx: &mut App) {
        let looping = turn(self.all(cx, PlayerView::is_looping));
        self.each(cx, |view, cx| view.turn_loop(|_| looping, cx));
    }

    /// Mute them all, or unmute them all, as `turn` has it, given whether
    /// all are muted.
    pub fn turn_mute(&self, turn: impl FnOnce(bool) -> bool, cx: &mut App) {
        let muted = turn(self.all(cx, PlayerView::is_muted));
        self.each(cx, |view, cx| view.turn_mute(|_| muted, cx));
    }
}

/// Every block to where `to` is, playing or paused as it is. It seeks
/// there too, so they all go on from the same time.
pub fn sync_to(to: &Entity<PlayerView>, cx: &mut App) {
    let (time, playing) = {
        let to = to.read(cx);
        (to.position(), to.is_playing())
    };
    Players::everywhere(cx).each(cx, |view, cx| {
        view.seek(time);
        if playing {
            view.play();
        } else {
            view.pause();
        }
        cx.notify();
    });
}
