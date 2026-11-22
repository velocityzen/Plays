//! A window's sound: through the app's output, or a device of its own,
//! picked from a tile's cast menu or Audio → Window Output Device. Its
//! tiles play there, the rest of the app's windows through the app's
//! output, and a tile moved to a new window takes the device along.
//!
//! A window Output Device puts full screen on another display plays
//! through that display's speakers, such as a TV's, until it comes back,
//! as Settings → Audio says: the output named after the display, as macOS
//! names an HDMI or DisplayPort output. Not one with an output picked for
//! it, and picking one there stops it.

use std::sync::Arc;

use gpui::{App, Context, Entity};

use super::Workspace;
use crate::app::{AudioEngine, refresh_menus};
use crate::media::Mixer;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, Settings};
use crate::ui::displays;
use crate::ui::player::PlayerView;

impl Workspace {
    /// The device it plays through instead of the app's output, if any.
    pub fn sound_device(&self) -> Option<&AudioDevice> {
        self.sound.as_ref().map(|(device, _)| device)
    }

    /// The mixer its tiles play into.
    pub(super) fn mixer(&self, cx: &App) -> Option<Arc<Mixer>> {
        match &self.sound {
            Some((_, mixer)) => Some(mixer.clone()),
            None => AudioEngine::mixer(cx),
        }
    }

    /// Play its tiles through `device` from now on, or with `None` through
    /// the app's output, as picked from a menu: its sound no longer follows
    /// the display it was sent to.
    pub fn set_sound_device(&mut self, device: Option<AudioDevice>, cx: &mut Context<Self>) {
        if let Some(home) = &mut self.home {
            home.sound = None;
        }
        self.play_through(device, cx);
    }

    /// Output Device is sending the window to display `id`: play through
    /// its speakers, unless Settings say not to or it plays through an
    /// output picked for it. Following another display's speakers already,
    /// it plays through what it did before where there are none.
    pub(super) fn follow_display(&mut self, id: u32, cx: &mut Context<Self>) {
        if !cx.global::<Settings>().sound_follows_display {
            return;
        }
        let Some(home) = &self.home else {
            return;
        };
        let before = match &home.sound {
            Some(before) => before.clone(),
            None if self.sound.is_none() => None,
            // Picked for it.
            None => return,
        };
        let following = home.sound.is_some();
        let speakers = displays::list(cx)
            .into_iter()
            .find(|display| display.id == id)
            .and_then(|display| speakers_of(&display.name, AudioEngine::devices(cx)))
            .map(|device| AudioDevice {
                id: device.id.clone(),
                name: device.name.clone(),
            });
        if speakers.is_none() && !following {
            return;
        }
        if let Some(home) = &mut self.home {
            home.sound = Some(before.clone());
        }
        self.play_through(speakers.or(before), cx);
    }

    /// Play its tiles through `device`, or with `None` through the app's
    /// output.
    pub(super) fn play_through(&mut self, device: Option<AudioDevice>, cx: &mut Context<Self>) {
        let id = |device: Option<&AudioDevice>| device.map(|device| device.id.clone());
        if id(self.sound_device()) == id(device.as_ref()) {
            return;
        }
        let old = self.sound.take();
        self.sound = device.and_then(|device| {
            let mixer = AudioEngine::window_mixer(&device, cx)?;
            Some((device, mixer))
        });
        for tile in &self.tiles {
            self.play_here(&tile.view, cx);
        }
        // Once its tiles have left it.
        if let Some((device, _)) = old {
            AudioEngine::let_go(&device, cx);
        }
        // Audio → Window Output Device checks it.
        refresh_menus(cx);
        cx.notify();
    }

    /// Have the tile of `view` play into its mixer, and know its device.
    pub(super) fn play_here(&self, view: &Entity<PlayerView>, cx: &mut App) {
        let (mixer, device) = (self.mixer(cx), self.sound_device().cloned());
        view.update(cx, |view, _| view.play_into(mixer, device));
    }

    /// Let go of its device's output as it closes.
    pub(super) fn let_go_of_sound(&mut self, cx: &mut App) {
        if let Some((device, _)) = self.sound.take() {
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
