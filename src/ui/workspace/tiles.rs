//! The tiles: adding and closing them, telling them where they are,
//! maximizing one, and moving one to a window of its own.

use gpui::{Context, Entity, EntityId, Pixels, Size, px, size};

use super::{TOOLBAR, Tile, Workspace};
use crate::media::Player;
use crate::ui::player::{PlayerEvent, PlayerView};
use crate::ui::theme::layout::{
    AUDIO_WINDOW_SIZE, TOOLBAR_HEIGHT, VIDEO_WINDOW_SIZE, WINDOW_MIN_SIZE, WINDOW_SIZE,
};

impl Workspace {
    pub(super) fn add_tile(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        let subscription = cx.subscribe(&view, |this, view, event, cx| match event {
            PlayerEvent::Close => this.close(view.entity_id(), cx),
            PlayerEvent::Activate => {
                this.active = Some(view.entity_id());
                this.tiles_changed(cx);
            }
            PlayerEvent::ToggleMaximize => this.toggle_maximize(view.entity_id(), cx),
            PlayerEvent::PopOut => this.pop_out(view.entity_id(), cx),
        });
        self.tiles.push(Tile {
            view,
            _subscription: subscription,
        });
    }

    /// Tell the tiles where they are and whether the mouse is in the window.
    /// The one in the top-left corner is under the traffic lights.
    pub(super) fn tiles_changed(&mut self, cx: &mut Context<Self>) {
        let alone = self.tiles.len() == 1;
        let mouse_in_window = self.mouse_inside;
        let top_left = self
            .maximized
            .or_else(|| self.tiles.first().map(|t| t.view.entity_id()));
        let active = self.active_view().map(|view| view.entity_id());
        for tile in &self.tiles {
            let is_top_left = top_left == Some(tile.view.entity_id());
            let is_active = active == Some(tile.view.entity_id());
            tile.view.update(cx, |view, cx| {
                if view.alone != alone
                    || view.top_left != is_top_left
                    || view.mouse_in_window != mouse_in_window
                    || view.active != is_active
                {
                    view.alone = alone;
                    view.top_left = is_top_left;
                    view.mouse_in_window = mouse_in_window;
                    view.active = is_active;
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    fn close(&mut self, id: EntityId, cx: &mut Context<Self>) {
        if let Some(index) = self.tiles.iter().position(|t| t.view.entity_id() == id) {
            let tile = self.tiles.remove(index);
            tile.view.update(cx, |view, _| view.release_pictures());
        }
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.tiles_changed(cx);
    }

    /// Move a tile to a new window, sized for it, where it keeps playing.
    fn pop_out(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(index) = self.tiles.iter().position(|t| t.view.entity_id() == id) else {
            return;
        };
        let view = self.tiles[index].view.clone();
        let window_size = window_size_for(view.read(cx).player());
        let window = match Self::open_window(window_size, cx) {
            Ok(window) => window,
            Err(err) => {
                log::error!("could not open a window: {err:#}");
                return;
            }
        };
        self.tiles.remove(index);
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.tiles_changed(cx);
        window
            .update(cx, |workspace, _, cx| workspace.adopt(view, cx))
            .ok();
    }

    /// Take in a tile from another window.
    fn adopt(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        let graveyard = self.graveyard.clone();
        view.update(cx, |view, _| view.move_to_window(graveyard));
        self.add_tile(view, cx);
        self.tiles_changed(cx);
    }

    pub fn close_all(&mut self, cx: &mut Context<Self>) {
        for tile in self.tiles.drain(..) {
            tile.view.update(cx, |view, _| view.release_pictures());
        }
        self.maximized = None;
        self.active = None;
        self.tiles_changed(cx);
    }

    fn toggle_maximize(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.maximized = if self.maximized == Some(id) {
            None
        } else {
            Some(id)
        };
        for tile in &self.tiles {
            let maximized = self.maximized == Some(tile.view.entity_id());
            tile.view.update(cx, |view, _| view.maximized = maximized);
        }
        self.tiles_changed(cx);
    }

    /// The tile shortcuts go to: the maximized one, else the one last
    /// clicked, else the first.
    fn active_view(&self) -> Option<Entity<PlayerView>> {
        let find = |id: EntityId| self.tiles.iter().find(|t| t.view.entity_id() == id);
        self.maximized
            .and_then(find)
            .or_else(|| self.active.and_then(find))
            .or_else(|| self.tiles.first())
            .map(|tile| tile.view.clone())
    }

    /// Run `f` on the tile shortcuts go to.
    pub fn with_active(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut PlayerView, &mut Context<PlayerView>),
    ) {
        if let Some(view) = self.active_view() {
            view.update(cx, f);
        }
    }
}

/// A window for one file: the video at its own size (scaled down to fit a
/// typical screen), a square for audio, and 16:9 while the file is still
/// opening.
fn window_size_for(player: &Player) -> Size<Pixels> {
    let info = player.info();
    let content = match info.as_ref().and_then(|i| i.video_size) {
        Some((width, height)) if width > 0 && height > 0 => {
            let (width, height) = (width as f32, height as f32);
            let scale = (WINDOW_SIZE.width / px(width))
                .min(WINDOW_SIZE.height / px(height))
                .min(1.);
            size(px(width * scale), px(height * scale))
        }
        _ if info.as_ref().is_some_and(|i| !i.has_video) => AUDIO_WINDOW_SIZE,
        _ => VIDEO_WINDOW_SIZE,
    };
    // The toolbar has a 1px border under it.
    let toolbar = if TOOLBAR {
        TOOLBAR_HEIGHT + px(1.)
    } else {
        px(0.)
    };
    size(
        content.width.max(WINDOW_MIN_SIZE.width),
        (content.height + toolbar).max(WINDOW_MIN_SIZE.height),
    )
}
