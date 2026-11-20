//! A window's sound: through the app's output, or a device of its own,
//! picked from a tile's Play On menu or Audio → Window Output Device. Its
//! tiles play there, the rest of the app's windows through the app's
//! output, and a tile moved to a new window takes the device along.

use std::sync::Arc;

use gpui::{App, Context, Entity};

use super::Workspace;
use crate::app::{AudioEngine, refresh_menus};
use crate::media::Mixer;
use crate::settings::AudioDevice;
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
    /// the app's output.
    pub fn set_sound_device(&mut self, device: Option<AudioDevice>, cx: &mut Context<Self>) {
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
