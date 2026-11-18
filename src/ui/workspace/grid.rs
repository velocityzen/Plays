//! The grid of tiles, in rows as Settings says, or the maximized tile alone.

use gpui::prelude::*;
use gpui::{Context, div};

use super::{Tile, Workspace};
use crate::settings::Settings;
use crate::ui::theme::layout::TILE_GAP;

impl Workspace {
    pub(super) fn render_grid(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let maximized = self
            .maximized
            .and_then(|id| self.tiles.iter().find(|t| t.view.entity_id() == id));
        let visible: Vec<&Tile> = match maximized {
            Some(tile) => vec![tile],
            None => self.tiles.iter().collect(),
        };
        let columns = cx
            .global::<Settings>()
            .columns_for(visible.len())
            .clamp(1, visible.len().max(1));

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(TILE_GAP)
            .children(visible.chunks(columns).map(|row| {
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(TILE_GAP)
                    .children(
                        row.iter()
                            .map(|tile| div().flex_1().min_w_0().h_full().child(tile.view.clone())),
                    )
                    .children((row.len()..columns).map(|_| div().flex_1().min_w_0()))
            }))
    }
}
