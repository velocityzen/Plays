//! The grid of tiles, in rows as Settings says, or the maximized tile alone.
//! A tile dragged by its picture onto another takes its place.

use gpui::prelude::*;
use gpui::{App, Context, Hsla, div};

use super::{Tile, Workspace};
use crate::settings::Settings;
use crate::ui::player::DraggedTile;
use crate::ui::theme::layout::TILE_GAP;

impl Workspace {
    pub(super) fn render_grid(
        &self,
        drop_highlight: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let maximized = self
            .maximized
            .and_then(|id| self.tiles.iter().find(|t| t.view.entity_id() == id));
        let visible: Vec<&Tile> = match maximized {
            Some(tile) => vec![tile],
            None => self.tiles.iter().collect(),
        };
        let columns = columns(visible.len(), cx);
        let reorder = visible.len() > 1;

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
                    .children(row.iter().map(|tile| {
                        // Tests find a tile by where it is in the window's
                        // list: "tile-0" first.
                        let id = tile.view.entity_id();
                        let index = self.tiles.iter().position(|t| t.view.entity_id() == id);
                        div()
                            .debug_selector(move || format!("tile-{}", index.unwrap_or_default()))
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(tile.view.clone())
                            // Over the tile, where another dragged onto it
                            // goes.
                            .when(reorder, |cell| {
                                cell.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .size_full()
                                        .drag_over::<DraggedTile>(move |style, dragged, _, _| {
                                            if dragged.0 == id {
                                                style
                                            } else {
                                                style.bg(drop_highlight)
                                            }
                                        })
                                        .on_drop(cx.listener(
                                            move |this, dragged: &DraggedTile, _, cx| {
                                                this.move_tile(dragged.0, id, cx);
                                            },
                                        )),
                                )
                            })
                    }))
                    .children((row.len()..columns).map(|_| div().flex_1().min_w_0()))
            }))
    }
}

/// How many columns `count` tiles are laid out in, as Settings say.
pub(super) fn columns(count: usize, cx: &App) -> usize {
    cx.global::<Settings>()
        .columns_for(count)
        .clamp(1, count.max(1))
}
