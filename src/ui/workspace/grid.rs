//! The grid of blocks, in rows as Settings says, or the maximized block alone.
//! A block dragged by its picture onto another takes its place.

use gpui::prelude::*;
use gpui::{App, Context, EntityId, Hsla, div};
use serde_json::json;

use super::{Block, Workspace};
use crate::settings::Settings;
use crate::ui::player::DraggedBlock;
use crate::ui::theme::layout::BLOCK_GAP;

impl Workspace {
    pub(super) fn render_grid(
        &self,
        drop_highlight: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let maximized = self
            .maximized
            .and_then(|id| self.blocks.iter().find(|t| t.view.entity_id() == id));
        let visible: Vec<&Block> = match maximized {
            Some(block) => vec![block],
            None => self.blocks.iter().collect(),
        };
        let columns = columns(visible.len(), cx);
        let reorder = visible.len() > 1;

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(BLOCK_GAP)
            .children(visible.chunks(columns).map(|row| {
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(BLOCK_GAP)
                    .children(row.iter().map(|block| {
                        // Tests find a block by where it is in the window's
                        // list: "block-0" first.
                        let id = block.view.entity_id();
                        let index = self.blocks.iter().position(|t| t.view.entity_id() == id);
                        div()
                            .debug_selector(move || format!("block-{}", index.unwrap_or_default()))
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(block.view.clone())
                            // Over the block, where another dragged onto it
                            // goes.
                            .when(reorder, |cell| {
                                cell.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .size_full()
                                        .drag_over::<DraggedBlock>(move |style, dragged, _, _| {
                                            if dragged.0 == id {
                                                style
                                            } else {
                                                style.bg(drop_highlight)
                                            }
                                        })
                                        .on_drop(cx.listener(
                                            move |this, dragged: &DraggedBlock, _, cx| {
                                                this.drop_block(dragged.0, id, cx);
                                            },
                                        )),
                                )
                            })
                    }))
                    .children((row.len()..columns).map(|_| div().flex_1().min_w_0()))
            }))
    }
}

impl Workspace {
    /// Move the block `id`, dropped on the block `onto`, to its place.
    fn drop_block(&self, id: EntityId, onto: EntityId, cx: &mut App) {
        let place = self.blocks.iter().position(|t| t.view.entity_id() == onto);
        if let Some(place) = place
            && id != onto
        {
            self.send_to_block(id, "block.move", json!({"position": place + 1}), cx);
        }
    }
}

/// How many columns `count` blocks are laid out in, as Settings say.
pub(super) fn columns(count: usize, cx: &App) -> usize {
    cx.global::<Settings>()
        .columns_for(count)
        .clamp(1, count.max(1))
}
