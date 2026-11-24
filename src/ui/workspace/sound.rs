//! Where a block's sound goes: through the app's output, or a device of
//! its own, picked from its menu or Audio → Block Output Device, which it
//! takes along to another window.
//!
//! A window Output Device puts full screen on another display plays
//! through that display's speakers, such as a TV's, until it comes back,
//! as Settings → Audio says: the output named after the display, as macOS
//! names an HDMI or DisplayPort output. Not its blocks with an output of
//! their own, picked before or while there.

use std::sync::Arc;

use gpui::{App, Context, Entity, EntityId};

use super::Workspace;
use crate::app::{AudioEngine, refresh_menus};
use crate::media::Mixer;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, Settings};
use crate::ui::displays;
use crate::ui::player::PlayerView;

impl Workspace {
    /// The speakers it plays through, sent to their display, if any.
    pub fn display_sound(&self) -> Option<&AudioDevice> {
        self.display_sound.as_ref().map(|(device, _)| device)
    }

    /// The mixer its blocks without a device of their own play into.
    pub(super) fn mixer(&self, cx: &App) -> Option<Arc<Mixer>> {
        match &self.display_sound {
            Some((_, mixer)) => Some(mixer.clone()),
            None => AudioEngine::mixer(cx),
        }
    }

    /// Play block `id` through `device` from now on, or with `None` as the
    /// window's other blocks do.
    pub fn set_block_sound(
        &mut self,
        id: EntityId,
        device: Option<AudioDevice>,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self
            .blocks
            .iter()
            .find(|block| block.view.entity_id() == id)
            .map(|block| block.view.clone())
        else {
            return;
        };
        let old = view.read(cx).sound_device().cloned();
        let ids = (old.as_ref(), device.as_ref());
        if ids.0.map(|old| &old.id) == ids.1.map(|device| &device.id) {
            return;
        }
        let own = device.and_then(|device| {
            let mixer = AudioEngine::device_mixer(&device, cx)?;
            Some((device, mixer))
        });
        let (mixer, device) = match own {
            Some((device, mixer)) => (Some(mixer), Some(device)),
            None => (self.mixer(cx), None),
        };
        view.update(cx, |view, _| view.play_into(mixer, device));
        // Once the block has left it.
        if let Some(old) = old {
            AudioEngine::let_go(&old, cx);
        }
        // Audio → Block Output Device checks it.
        refresh_menus(cx);
        cx.notify();
    }

    /// Output Device is sending the window to display `id`: play through
    /// its speakers, unless Settings say not to; following another
    /// display's speakers already, through the app's output where there
    /// are none.
    pub(super) fn follow_display(&mut self, id: u32, cx: &mut Context<Self>) {
        if !cx.global::<Settings>().sound_follows_display || self.home.is_none() {
            return;
        }
        let speakers = displays::list(cx)
            .into_iter()
            .find(|display| display.id == id)
            .and_then(|display| speakers_of(&display.name, AudioEngine::devices(cx)))
            .map(|device| AudioDevice {
                id: device.id.clone(),
                name: device.name.clone(),
            });
        if speakers.is_some() || self.display_sound.is_some() {
            self.play_through_display(speakers, cx);
        }
    }

    /// Play its blocks without a device of their own through `device`, a
    /// display's speakers, or with `None` through the app's output.
    pub(super) fn play_through_display(
        &mut self,
        device: Option<AudioDevice>,
        cx: &mut Context<Self>,
    ) {
        let id = |device: Option<&AudioDevice>| device.map(|device| device.id.clone());
        if id(self.display_sound()) == id(device.as_ref()) {
            return;
        }
        let old = self.display_sound.take();
        self.display_sound = device.and_then(|device| {
            let mixer = AudioEngine::device_mixer(&device, cx)?;
            Some((device, mixer))
        });
        for block in &self.blocks {
            self.play_here(&block.view, cx);
        }
        // Once its blocks have left it.
        if let Some((device, _)) = old {
            AudioEngine::let_go(&device, cx);
        }
        cx.notify();
    }

    /// Have the block of `view` play into the window's mixer, unless it
    /// plays through a device of its own.
    pub(super) fn play_here(&self, view: &Entity<PlayerView>, cx: &mut App) {
        if view.read(cx).sound_device().is_none() {
            let mixer = self.mixer(cx);
            view.update(cx, |view, _| view.play_into(mixer, None));
        }
    }

    /// What picked output `id` no longer does: its blocks playing through
    /// it play as the others do, and following a display's speakers that
    /// are it, they play through the app's output.
    pub fn unpick(&mut self, id: &str, cx: &mut Context<Self>) {
        let picked: Vec<EntityId> = self
            .blocks
            .iter()
            .filter(|block| {
                let own = block.view.read(cx).sound_device();
                own.is_some_and(|own| own.id == id)
            })
            .map(|block| block.view.entity_id())
            .collect();
        for block in picked {
            self.set_block_sound(block, None, cx);
        }
        if self.display_sound().is_some_and(|device| device.id == id) {
            self.play_through_display(None, cx);
        }
    }

    /// Let go of the output the block of `view` plays through of its own,
    /// as it closes.
    pub(super) fn let_go_of_block_sound(view: &Entity<PlayerView>, cx: &mut App) {
        if let Some(device) = view.read(cx).sound_device().cloned() {
            AudioEngine::let_go(&device, cx);
        }
    }

    /// Let go of the outputs it and its blocks play through as it closes.
    pub(super) fn let_go_of_sound(&mut self, cx: &mut App) {
        for block in &self.blocks {
            Self::let_go_of_block_sound(&block.view, cx);
        }
        if let Some((device, _)) = self.display_sound.take() {
            AudioEngine::let_go(&device, cx);
        }
    }
}

/// The words of a name, to match by: "LG TV SSCR2" is `lg tv sscr2`.
fn words(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether `words` has `part` in it, word for word.
fn has_words(words: &[String], part: &[String]) -> bool {
    !part.is_empty() && words.windows(part.len()).any(|run| run == part)
}

/// The output that is display `name`'s speakers: the one named as it is,
/// or else the only one whose name has the display's in it, or is in the
/// display's, word for word ("LG TV" for "LG TV SSCR2", "Studio Display
/// Speakers" for "Studio Display").
fn speakers_of<'a>(name: &str, devices: &'a [DeviceInfo]) -> Option<&'a DeviceInfo> {
    let display = words(name);
    if display.is_empty() {
        return None;
    }
    let named = |device: &&DeviceInfo| words(&device.name) == display;
    if let Some(device) = devices.iter().find(named) {
        return Some(device);
    }
    let mut alike = devices.iter().filter(|device| {
        let device = words(&device.name);
        has_words(&device, &display) || has_words(&display, &device)
    });
    match (alike.next(), alike.next()) {
        (Some(device), None) => Some(device),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/sound.rs"]
mod tests;
