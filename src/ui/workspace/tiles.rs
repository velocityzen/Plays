//! The tiles: adding and closing them, telling them where they are,
//! putting one in another's place, maximizing one, and moving one to a
//! window of its own.

use gpui::{App, Context, Entity, EntityId, Pixels, Size, px, size};

use super::{TOOLBAR_SPACE, Tile, Workspace};
use crate::app::{Awake, refresh_menus};
use crate::cast::Device;
use crate::progress::{self, Note};
use crate::settings::AudioDevice;
use crate::ui::displays;
use crate::ui::framing::Adjustments;
use crate::ui::player::{PlayerEvent, PlayerView};
use crate::ui::theme::layout::{
    AUDIO_WINDOW_SIZE, VIDEO_WINDOW_SIZE, WINDOW_MIN_SIZE, WINDOW_SIZE,
};

impl Workspace {
    pub(super) fn add_tile(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        self.play_here(&view, cx);
        let subscription = cx.subscribe(&view, |this, view, event, cx| match event {
            PlayerEvent::Close => this.close(view.entity_id(), cx),
            PlayerEvent::Activate => {
                this.active = Some(view.entity_id());
                this.tiles_changed(cx);
                // Video → Aspect Ratio and Crop check this one's now.
                refresh_menus(cx);
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
        let (mouse_in_window, mouse_idle) = (self.mouse_inside, self.mouse_idle);
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
                    || view.mouse_idle != mouse_idle
                    || view.active != is_active
                {
                    view.alone = alone;
                    view.top_left = is_top_left;
                    view.mouse_in_window = mouse_in_window;
                    view.mouse_idle = mouse_idle;
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
            let note = tile.view.read(cx).progress();
            progress::note(note, true, cx);
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

    /// Move a tile to a new window, sized for it, where it keeps playing:
    /// beside this one, on its display.
    fn pop_out(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(index) = self.tiles.iter().position(|t| t.view.entity_id() == id) else {
            return;
        };
        let view = self.tiles[index].view.clone();
        let window_size = window_size_for(view.read(cx));
        let here = self
            .window
            .update(cx, |_, window, cx| {
                Some((window.display(cx)?, window.bounds()))
            })
            .ok()
            .flatten();
        let opened = match here {
            Some((display, from)) => {
                let bounds = displays::beside(from, window_size, display.bounds());
                Self::open_window_at(Some(display.id()), bounds, cx)
            }
            None => Self::open_window(window_size, cx),
        };
        let window = match opened {
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
        let sound = self.sound_device().cloned();
        window
            .update(cx, |workspace, _, cx| workspace.adopt(view, sound, cx))
            .ok();
    }

    /// Take in a tile from another window, with the device that window
    /// plays through instead of the app's output.
    fn adopt(
        &mut self,
        view: Entity<PlayerView>,
        sound: Option<AudioDevice>,
        cx: &mut Context<Self>,
    ) {
        self.set_sound_device(sound, cx);
        let graveyard = self.graveyard.clone();
        view.update(cx, |view, _| view.move_to_window(graveyard));
        self.add_tile(view, cx);
        self.tiles_changed(cx);
    }

    /// Close the tile shortcuts go to; the window stays, empty after the
    /// last.
    pub fn close_active(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.active_view() {
            self.close(view.entity_id(), cx);
        }
    }

    pub fn close_all(&mut self, cx: &mut Context<Self>) {
        let notes = self.progress(cx);
        progress::note(notes, true, cx);
        for tile in self.tiles.drain(..) {
            tile.view.update(cx, |view, _| view.release_pictures());
        }
        self.maximized = None;
        self.active = None;
        self.tiles_changed(cx);
    }

    /// Put the tile `id` where the tile `onto` is, the tiles between
    /// moving over, as a tile dragged onto another goes. Shortcuts go to it
    /// there, under the mouse.
    pub(super) fn move_tile(&mut self, id: EntityId, onto: EntityId, cx: &mut Context<Self>) {
        let place = |id| self.tiles.iter().position(|t| t.view.entity_id() == id);
        if let (Some(from), Some(to)) = (place(id), place(onto))
            && from != to
        {
            let tile = self.tiles.remove(from);
            self.tiles.insert(to, tile);
            self.active = Some(id);
            self.tiles_changed(cx);
            refresh_menus(cx);
        }
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

    /// The tile shortcuts go to: the maximized one, else the one the mouse
    /// was last over (or clicked), else the first.
    pub(super) fn active_view(&self) -> Option<Entity<PlayerView>> {
        let find = |id: EntityId| self.tiles.iter().find(|t| t.view.entity_id() == id);
        self.maximized
            .and_then(find)
            .or_else(|| self.active.and_then(find))
            .or_else(|| self.tiles.first())
            .map(|tile| tile.view.clone())
    }

    /// What its players keep awake: the screen while a video plays, the
    /// computer while only sound does, or a tile casts (the media is
    /// served from here).
    pub fn keeps_awake(&self, cx: &App) -> Awake {
        self.tiles
            .iter()
            .map(|tile| {
                let view = tile.view.read(cx);
                let player = view.player();
                if view.casting().is_some() {
                    Awake::Computer
                } else if !player.is_playing() {
                    Awake::Nothing
                } else if player.info().is_some_and(|info| info.has_video) {
                    Awake::Screen
                } else {
                    Awake::Computer
                }
            })
            .max()
            .unwrap_or_default()
    }

    /// How the tile shortcuts go to is turned, shaped and cropped.
    pub fn active_adjustments(&self, cx: &App) -> Adjustments {
        self.active_view()
            .map(|view| view.read(cx).adjustments())
            .unwrap_or_default()
    }

    /// The names of the chapters of the tile shortcuts go to, and the one
    /// it's in.
    pub fn active_chapters(&self, cx: &App) -> (Vec<String>, Option<usize>) {
        self.active_view().map_or((Vec::new(), None), |view| {
            let view = view.read(cx);
            let names = view
                .chapters()
                .iter()
                .enumerate()
                .map(|(index, chapter)| chapter.name(index))
                .collect();
            (names, view.chapter())
        })
    }

    /// Where its tiles' files are, for resuming them.
    pub fn progress(&self, cx: &App) -> Vec<Note> {
        self.tiles
            .iter()
            .filter_map(|tile| tile.view.read(cx).progress())
            .collect()
    }

    /// The device the tile shortcuts go to casts to.
    pub fn active_casting(&self, cx: &App) -> Option<Device> {
        self.active_view()?.read(cx).casting().cloned()
    }

    /// Whether the tile shortcuts go to has sound, or isn't yet known to
    /// have none; with no tile, whether one would.
    pub fn active_has_audio(&self, cx: &App) -> bool {
        self.active_view().is_none_or(|view| {
            view.read(cx)
                .player()
                .info()
                .is_none_or(|info| info.has_audio)
        })
    }

    /// Play the tile shortcuts go to here again, if it casts; whether it
    /// did.
    pub fn end_active_cast(&mut self, cx: &mut Context<Self>) -> bool {
        self.active_view()
            .is_some_and(|view| view.update(cx, |view, cx| view.end_cast(cx)))
    }

    /// Stop every device its tiles cast to, as the app quits, handing back
    /// the threads stopping them.
    pub fn stop_casts(&mut self, cx: &mut Context<Self>) -> Vec<std::thread::JoinHandle<()>> {
        self.tiles
            .iter()
            .filter_map(|tile| tile.view.update(cx, |view, _| view.stop_cast()))
            .collect()
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

/// A window for one file: the video at the size it shows (scaled down to
/// fit a typical screen), a square for audio, and 16:9 while the file is
/// still opening.
fn window_size_for(view: &PlayerView) -> Size<Pixels> {
    let info = view.player().info();
    let content = match view.framing() {
        Some(framing) => {
            let (width, height) = framing.size;
            let scale = (WINDOW_SIZE.width / px(width))
                .min(WINDOW_SIZE.height / px(height))
                .min(1.);
            size(px(width * scale), px(height * scale))
        }
        _ if info.as_ref().is_some_and(|i| !i.has_video) => AUDIO_WINDOW_SIZE,
        _ => VIDEO_WINDOW_SIZE,
    };
    size(
        content.width.max(WINDOW_MIN_SIZE.width),
        (content.height + TOOLBAR_SPACE).max(WINDOW_MIN_SIZE.height),
    )
}
