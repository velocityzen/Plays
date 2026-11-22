//! The blocks: adding and closing them, telling them where they are,
//! putting one in another's place, maximizing one, and moving one to
//! another window or one of its own.

use anyhow::Result;
use gpui::{App, Context, Entity, EntityId, Pixels, Size, Window, WindowHandle, px, size};
use serde_json::Value;

use super::{Block, TOOLBAR_SPACE, Workspace};
use crate::api::{self, Invocation};
use crate::app::{Awake, refresh_menus};
use crate::cast::Device;
use crate::progress::{self, Note};
use crate::settings::AudioDevice;
use crate::ui::displays;
use crate::ui::player::{BlockLayout, PlayerEvent, PlayerView};
use crate::ui::theme::layout::{
    AUDIO_WINDOW_SIZE, VIDEO_WINDOW_SIZE, WINDOW_MIN_SIZE, WINDOW_SIZE,
};

impl Workspace {
    pub(super) fn add_block(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        self.play_here(&view, cx);
        let subscription = cx.subscribe(&view, |this, view, event, cx| match event {
            PlayerEvent::Activate => {
                this.active = Some(view.entity_id());
                this.keyboard_focus = false;
                this.blocks_changed(cx);
                // Video → Aspect Ratio and Crop check this one's now.
                refresh_menus(cx);
            }
            PlayerEvent::Pressed => this.note_front_block(view.entity_id()),
        });
        self.blocks.push(Block {
            view,
            _subscription: subscription,
        });
    }

    /// Tell the blocks where they are and whether the mouse is in the window.
    /// The one in the top-left corner is under the traffic lights.
    pub(super) fn blocks_changed(&mut self, cx: &mut Context<Self>) {
        let alone = self.blocks.len() == 1;
        let top_left = self
            .maximized
            .or_else(|| self.blocks.first().map(|t| t.view.entity_id()));
        let active = self.active_view().map(|view| view.entity_id());
        // A ring tells which block the keys went to, among others.
        let ring = self.keyboard_focus && self.maximized.is_none() && !alone;
        for block in &self.blocks {
            let id = Some(block.view.entity_id());
            let layout = BlockLayout {
                active: active == id,
                focus_ring: ring && active == id,
                maximized: self.maximized == id,
                alone,
                top_left: top_left == id,
                mouse_in_window: self.mouse_inside,
                mouse_idle: self.mouse_idle,
            };
            block
                .view
                .update(cx, |view, cx| view.set_layout(layout, cx));
        }
        cx.notify();
    }

    /// Close the block `id`; the window stays, empty after the last.
    pub fn close(&mut self, id: EntityId, cx: &mut Context<Self>) {
        if let Some(index) = self.blocks.iter().position(|t| t.view.entity_id() == id) {
            let block = self.blocks.remove(index);
            let note = block.view.read(cx).progress();
            progress::note(note, true, cx);
            block.view.update(cx, |view, _| view.release_pictures());
        }
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.blocks_changed(cx);
    }

    /// Run `command` on the block `id`, given `args` (null for none), once
    /// the event is handled: what the window does to its blocks, a drop say.
    pub(super) fn send_to_block(&self, id: EntityId, command: &str, args: Value, cx: &mut App) {
        let Some(block) = self
            .blocks
            .iter()
            .find(|block| block.view.entity_id() == id)
        else {
            return;
        };
        let name = block.view.read(cx).id().to_string();
        api::dispatch(Invocation::new(command).with_args(args).on_block(name), cx);
    }

    /// Run `command` in this window, given `args` (null for none), once the
    /// event is handled: what its controls do.
    pub(super) fn send(&self, command: &str, args: Value, cx: &mut App) {
        let invocation = Invocation::new(command)
            .with_args(args)
            .in_window(self.id().to_string());
        api::dispatch(invocation, cx);
    }

    /// Move the block `id` to a new window, sized for it, where it keeps
    /// playing through what this one plays through: beside this one, on
    /// its display. The window, or why none opened.
    pub fn move_to_new_window(
        &mut self,
        id: EntityId,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<WindowHandle<Self>>> {
        let Some(block) = self.blocks.iter().find(|t| t.view.entity_id() == id) else {
            return Ok(None);
        };
        let window_size = window_size_for(block.view.read(cx));
        let here = window.display(cx).map(|display| (display, window.bounds()));
        let opened = match here {
            Some((display, from)) => {
                let bounds = displays::beside(from, window_size, display.bounds());
                Self::open_window_at(Some(display.id()), bounds, cx)
            }
            None => Self::open_window(window_size, cx),
        };
        let opened = opened?;
        let Some(view) = self.release_block(id, cx) else {
            return Ok(None);
        };
        let sound = self.sound_device().cloned();
        opened.update(cx, |workspace, _, cx| workspace.adopt(view, sound, cx))?;
        Ok(Some(opened))
    }

    /// Let go of the block `id`, for another window to take in (`take_block`).
    pub fn release_block(
        &mut self,
        id: EntityId,
        cx: &mut Context<Self>,
    ) -> Option<Entity<PlayerView>> {
        let index = self.blocks.iter().position(|t| t.view.entity_id() == id)?;
        let block = self.blocks.remove(index);
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.blocks_changed(cx);
        Some(block.view)
    }

    /// Take in a block another window let go of, playing on through what
    /// this one plays through, last among them; keys go to it.
    pub fn take_block(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        let graveyard = self.graveyard.clone();
        view.update(cx, |view, _| view.move_to_window(graveyard));
        self.active = Some(view.entity_id());
        self.note_front_block(view.entity_id());
        self.add_block(view, cx);
        self.blocks_changed(cx);
        refresh_menus(cx);
    }

    /// Take in a block from another window, with the device that window
    /// plays through instead of the app's output.
    fn adopt(
        &mut self,
        view: Entity<PlayerView>,
        sound: Option<AudioDevice>,
        cx: &mut Context<Self>,
    ) {
        self.set_sound_device(sound, cx);
        self.take_block(view, cx);
    }

    /// Close every block but `id`.
    pub fn close_others(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let others: Vec<EntityId> = self
            .blocks
            .iter()
            .map(|block| block.view.entity_id())
            .filter(|&other| other != id)
            .collect();
        for other in others {
            self.close(other, cx);
        }
    }

    pub fn close_all(&mut self, cx: &mut Context<Self>) {
        let notes = self.progress(cx);
        progress::note(notes, true, cx);
        for block in self.blocks.drain(..) {
            block.view.update(cx, |view, _| view.release_pictures());
        }
        self.maximized = None;
        self.active = None;
        self.blocks_changed(cx);
    }

    /// Put the block `id` at `place` among them, from 0, the blocks between
    /// moving over, as a block dragged onto another goes; past the last is
    /// last. Shortcuts go to it there.
    pub fn move_block_to(&mut self, id: EntityId, place: usize, cx: &mut Context<Self>) {
        let Some(from) = self.blocks.iter().position(|t| t.view.entity_id() == id) else {
            return;
        };
        let to = place.min(self.blocks.len() - 1);
        if from != to {
            let block = self.blocks.remove(from);
            self.blocks.insert(to, block);
            self.active = Some(id);
            self.blocks_changed(cx);
            refresh_menus(cx);
        }
    }

    /// Show the block `id` alone, or every block again if it is; a window's
    /// only block fills the screen instead, or no longer does.
    pub fn zoom(&mut self, id: EntityId, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocks.len() == 1 {
            window.toggle_fullscreen();
        } else {
            self.maximized = (self.maximized != Some(id)).then_some(id);
            self.blocks_changed(cx);
        }
    }

    /// How many blocks it has.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// The block shortcuts go to: the maximized one, else the one the mouse
    /// was last over (or clicked), else the first.
    pub(super) fn active_view(&self) -> Option<Entity<PlayerView>> {
        let find = |id: EntityId| self.blocks.iter().find(|t| t.view.entity_id() == id);
        self.maximized
            .and_then(find)
            .or_else(|| self.active.and_then(find))
            .or_else(|| self.blocks.first())
            .map(|block| block.view.clone())
    }

    /// What its players keep awake: the screen while a video plays, the
    /// computer while only sound does, or a block casts (the media is
    /// served from here).
    pub fn keeps_awake(&self, cx: &App) -> Awake {
        self.blocks
            .iter()
            .map(|block| {
                let view = block.view.read(cx);
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

    /// The names of the chapters of the block shortcuts go to, and the one
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

    /// Where its blocks' files are, for resuming them.
    pub fn progress(&self, cx: &App) -> Vec<Note> {
        self.blocks
            .iter()
            .filter_map(|block| block.view.read(cx).progress())
            .collect()
    }

    /// The device the block shortcuts go to casts to.
    pub fn active_casting(&self, cx: &App) -> Option<Device> {
        self.active_view()?.read(cx).casting().cloned()
    }

    /// Whether the block shortcuts go to has sound, or isn't yet known to
    /// have none; with no block, whether one would.
    pub fn active_has_audio(&self, cx: &App) -> bool {
        self.active_view().is_none_or(|view| {
            view.read(cx)
                .player()
                .info()
                .is_none_or(|info| info.has_audio)
        })
    }

    /// Play the block shortcuts go to here again, if it casts; whether it
    /// did.
    pub fn end_active_cast(&mut self, cx: &mut Context<Self>) -> bool {
        self.active_view()
            .is_some_and(|view| view.update(cx, |view, cx| view.end_cast(cx)))
    }

    /// Stop every device its blocks cast to, as the app quits or the window
    /// closes, handing back the threads stopping them.
    pub fn stop_casts(&mut self, cx: &mut App) -> Vec<std::thread::JoinHandle<()>> {
        self.blocks
            .iter()
            .filter_map(|block| block.view.update(cx, |view, _| view.stop_cast()))
            .collect()
    }

    /// Run `f` on the block shortcuts go to.
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
