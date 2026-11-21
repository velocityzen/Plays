//! The tiles: adding and closing them, telling them where they are,
//! putting one in another's place, maximizing one, and moving one to
//! another window or one of its own.

use anyhow::Result;
use gpui::{App, Context, Entity, EntityId, Pixels, Size, Window, WindowHandle, px, size};
use serde_json::Value;

use super::{TOOLBAR_SPACE, Tile, Workspace};
use crate::api::{self, Invocation};
use crate::app::{Awake, refresh_menus};
use crate::cast::Device;
use crate::progress::{self, Note};
use crate::settings::AudioDevice;
use crate::ui::displays;
use crate::ui::player::{PlayerEvent, PlayerView, TileLayout};
use crate::ui::theme::layout::{
    AUDIO_WINDOW_SIZE, VIDEO_WINDOW_SIZE, WINDOW_MIN_SIZE, WINDOW_SIZE,
};

impl Workspace {
    pub(super) fn add_tile(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        self.play_here(&view, cx);
        let subscription = cx.subscribe(&view, |this, view, event, cx| match event {
            PlayerEvent::Activate => {
                this.active = Some(view.entity_id());
                this.keyboard_focus = false;
                this.tiles_changed(cx);
                // Video → Aspect Ratio and Crop check this one's now.
                refresh_menus(cx);
            }
            PlayerEvent::Pressed => this.note_front_tile(view.entity_id()),
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
        let top_left = self
            .maximized
            .or_else(|| self.tiles.first().map(|t| t.view.entity_id()));
        let active = self.active_view().map(|view| view.entity_id());
        // A ring tells which tile the keys went to, among others.
        let ring = self.keyboard_focus && self.maximized.is_none() && !alone;
        for tile in &self.tiles {
            let id = Some(tile.view.entity_id());
            let layout = TileLayout {
                active: active == id,
                focus_ring: ring && active == id,
                maximized: self.maximized == id,
                alone,
                top_left: top_left == id,
                mouse_in_window: self.mouse_inside,
                mouse_idle: self.mouse_idle,
            };
            tile.view.update(cx, |view, cx| view.set_layout(layout, cx));
        }
        cx.notify();
    }

    /// Close the tile `id`; the window stays, empty after the last.
    pub fn close(&mut self, id: EntityId, cx: &mut Context<Self>) {
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

    /// Run `command` on the tile `id`, given `args` (null for none), once
    /// the event is handled: what the window does to its tiles, a drop say.
    pub(super) fn send_to_tile(&self, id: EntityId, command: &str, args: Value, cx: &mut App) {
        let Some(tile) = self.tiles.iter().find(|tile| tile.view.entity_id() == id) else {
            return;
        };
        let name = tile.view.read(cx).id().to_string();
        api::dispatch(Invocation::new(command).with_args(args).on_tile(name), cx);
    }

    /// Run `command` in this window, given `args` (null for none), once the
    /// event is handled: what its controls do.
    pub(super) fn send(&self, command: &str, args: Value, cx: &mut App) {
        let invocation = Invocation::new(command)
            .with_args(args)
            .in_window(self.id().to_string());
        api::dispatch(invocation, cx);
    }

    /// Move the tile `id` to a new window, sized for it, where it keeps
    /// playing through what this one plays through: beside this one, on
    /// its display. The window, or why none opened.
    pub fn move_to_new_window(
        &mut self,
        id: EntityId,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<WindowHandle<Self>>> {
        let Some(tile) = self.tiles.iter().find(|t| t.view.entity_id() == id) else {
            return Ok(None);
        };
        let window_size = window_size_for(tile.view.read(cx));
        let here = window.display(cx).map(|display| (display, window.bounds()));
        let opened = match here {
            Some((display, from)) => {
                let bounds = displays::beside(from, window_size, display.bounds());
                Self::open_window_at(Some(display.id()), bounds, cx)
            }
            None => Self::open_window(window_size, cx),
        };
        let opened = opened?;
        let Some(view) = self.release_tile(id, cx) else {
            return Ok(None);
        };
        let sound = self.sound_device().cloned();
        opened.update(cx, |workspace, _, cx| workspace.adopt(view, sound, cx))?;
        Ok(Some(opened))
    }

    /// Let go of the tile `id`, for another window to take in (`take_tile`).
    pub fn release_tile(
        &mut self,
        id: EntityId,
        cx: &mut Context<Self>,
    ) -> Option<Entity<PlayerView>> {
        let index = self.tiles.iter().position(|t| t.view.entity_id() == id)?;
        let tile = self.tiles.remove(index);
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.tiles_changed(cx);
        Some(tile.view)
    }

    /// Take in a tile another window let go of, playing on through what
    /// this one plays through, last among them; keys go to it.
    pub fn take_tile(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        let graveyard = self.graveyard.clone();
        view.update(cx, |view, _| view.move_to_window(graveyard));
        self.active = Some(view.entity_id());
        self.note_front_tile(view.entity_id());
        self.add_tile(view, cx);
        self.tiles_changed(cx);
        refresh_menus(cx);
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
        self.take_tile(view, cx);
    }

    /// Close every tile but `id`.
    pub fn close_others(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let others: Vec<EntityId> = self
            .tiles
            .iter()
            .map(|tile| tile.view.entity_id())
            .filter(|&other| other != id)
            .collect();
        for other in others {
            self.close(other, cx);
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

    /// Put the tile `id` at `place` among them, from 0, the tiles between
    /// moving over, as a tile dragged onto another goes; past the last is
    /// last. Shortcuts go to it there.
    pub fn move_tile_to(&mut self, id: EntityId, place: usize, cx: &mut Context<Self>) {
        let Some(from) = self.tiles.iter().position(|t| t.view.entity_id() == id) else {
            return;
        };
        let to = place.min(self.tiles.len() - 1);
        if from != to {
            let tile = self.tiles.remove(from);
            self.tiles.insert(to, tile);
            self.active = Some(id);
            self.tiles_changed(cx);
            refresh_menus(cx);
        }
    }

    /// Show the tile `id` alone, or every tile again if it is; a window's
    /// only tile fills the screen instead, or no longer does.
    pub fn zoom(&mut self, id: EntityId, window: &mut Window, cx: &mut Context<Self>) {
        if self.tiles.len() == 1 {
            window.toggle_fullscreen();
        } else {
            self.maximized = (self.maximized != Some(id)).then_some(id);
            self.tiles_changed(cx);
        }
    }

    /// How many tiles it has.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
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
                } else if player.has_video() {
                    Awake::Screen
                } else {
                    Awake::Computer
                }
            })
            .max()
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

    /// Stop every device its tiles cast to, as the app quits or the window
    /// closes, handing back the threads stopping them.
    pub fn stop_casts(&mut self, cx: &mut App) -> Vec<std::thread::JoinHandle<()>> {
        self.tiles
            .iter()
            .filter_map(|tile| tile.view.update(cx, |view, _| view.stop_cast()))
            .collect()
    }

    /// Run `f` on the tile shortcuts go to.
    #[cfg(test)]
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
