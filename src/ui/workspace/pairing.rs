//! Pairing with the TV a tile was cast to, which asked for the PIN it
//! shows: a sheet over the window, and the tile cast there again once it's
//! paired.

use gpui::{AppContext as _, Context, Entity, EntityId, Subscription};

use super::Workspace;
use crate::cast::{Casting, Device};
use crate::ui::pairing::{PairingDialog, PairingEvent};
use crate::ui::player::PlayerView;

/// The sheet, and the tile to cast once paired.
pub(super) struct Pairing {
    pub(super) dialog: Entity<PairingDialog>,
    tile: EntityId,
    _events: Subscription,
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
        let dialog = cx.new(|cx| PairingDialog::new(device, target, cx));
        let events = cx.subscribe(&dialog, |this, _, event: &PairingEvent, cx| {
            let Some(pairing) = this.pairing.take() else {
                return;
            };
            let tile = this
                .tiles
                .iter()
                .find(|tile| tile.view.entity_id() == pairing.tile)
                .map(|tile| tile.view.clone());
            if let (PairingEvent::Paired(device), Some(tile)) = (event, tile) {
                let device = device.clone();
                tile.update(cx, |view, cx| view.cast_to(Some(device), cx));
            }
            this.refocus = true;
            cx.notify();
        });
        self.pairing = Some(Pairing {
            dialog,
            tile: view.entity_id(),
            _events: events,
        });
        cx.notify();
    }

    /// The sheet asking for a TV's PIN, while it's up.
    #[cfg(test)]
    pub fn pairing(&self) -> Option<Entity<PairingDialog>> {
        self.pairing.as_ref().map(|pairing| pairing.dialog.clone())
    }
}
