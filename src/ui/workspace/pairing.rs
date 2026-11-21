//! Pairing with what asked for the PIN it shows: the TV a tile was cast
//! to, cast there again once it's paired, or a speaker the sound was to
//! play through, played through then, and not paired with, no longer
//! picked. A sheet over the window.

use std::sync::Arc;

use gpui::{AppContext as _, Context, Entity, EntityId, Subscription};

use super::Workspace;
use crate::app::AudioEngine;
use crate::cast::{Casting, Device};
use crate::settings::AudioDevice;
use crate::ui::pairing::{PairingDialog, PairingEvent, StartPairing};
use crate::ui::player::PlayerView;

/// The sheet, and what's done once paired.
pub(super) struct Pairing {
    pub(super) dialog: Entity<PairingDialog>,
    then: Then,
    _events: Subscription,
}

/// What's done once paired.
enum Then {
    /// Cast the tile to the device again.
    Cast { tile: EntityId, device: Device },
    /// Play the audio outputs that want this device through it; not
    /// paired with, they no longer want it.
    Play(AudioDevice),
}

impl Workspace {
    /// Pair with `device`, which the tile `view` was cast to; it's cast
    /// there again, from where it is then, once paired. Another TV asked
    /// for meanwhile takes the sheet's place.
    pub(super) fn pair(
        &mut self,
        view: &Entity<PlayerView>,
        device: Device,
        cx: &mut Context<Self>,
    ) {
        let target = match Casting::target(cx, &device) {
            Ok(target) => target,
            Err(why) => {
                let note = format!("Can't pair with {}: {why}", device.name);
                view.update(cx, |view, cx| view.note(note, cx));
                return;
            }
        };
        let start: StartPairing = {
            let device = device.clone();
            Arc::new(move || target.pair(&device))
        };
        let tile = view.entity_id();
        self.ask_for_pin(device.name.clone(), start, Then::Cast { tile, device }, cx);
    }

    /// Pair with audio output `device`, which wants the PIN it shows,
    /// starting with `start`; the outputs that want it play through it once
    /// paired. Not while another sheet is up.
    pub fn pair_output(
        &mut self,
        device: AudioDevice,
        start: StartPairing,
        cx: &mut Context<Self>,
    ) {
        if self.pairing.is_none() {
            self.ask_for_pin(device.name.clone(), start, Then::Play(device), cx);
        }
    }

    fn ask_for_pin(
        &mut self,
        name: String,
        start: StartPairing,
        then: Then,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| PairingDialog::new(name, start, cx));
        let events = cx.subscribe(&dialog, |this, _, event: &PairingEvent, cx| {
            let Some(pairing) = this.pairing.take() else {
                return;
            };
            match (event, pairing.then) {
                (PairingEvent::Paired, Then::Cast { tile, device }) => {
                    let tile = this
                        .tiles
                        .iter()
                        .find(|found| found.view.entity_id() == tile)
                        .map(|found| found.view.clone());
                    if let Some(tile) = tile {
                        tile.update(cx, |view, cx| view.cast_to(Some(device), cx));
                    }
                }
                (PairingEvent::Paired, Then::Play(_)) => AudioEngine::paired(cx),
                // Not paired with: the sound doesn't wait for it, silent.
                // Once this window is done updating, as it may be one.
                (PairingEvent::Dismiss, Then::Play(device)) => {
                    cx.defer(move |cx| AudioEngine::not_paired(&device, cx));
                }
                (PairingEvent::Dismiss, Then::Cast { .. }) => {}
            }
            this.refocus = true;
            cx.notify();
        });
        self.pairing = Some(Pairing {
            dialog,
            then,
            _events: events,
        });
        cx.notify();
    }

    /// The sheet asking for a PIN, while it's up.
    #[cfg(test)]
    pub fn pairing(&self) -> Option<Entity<PairingDialog>> {
        self.pairing.as_ref().map(|pairing| pairing.dialog.clone())
    }
}
