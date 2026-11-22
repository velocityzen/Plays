//! Going from block to block, and moving blocks about the grid, from the
//! keyboard: to the block beside, above or below the one in front, or the
//! one before or after it in the window's order. The block focused so shows
//! a ring until the mouse moves, as the block under the mouse is in front
//! from then on.

use gpui::{App, Context, EntityId};

use super::Workspace;
use super::grid::columns;
use crate::api;
use crate::app::refresh_menus;

/// Where to go from the block in front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
    Up,
    Down,
    Previous,
    Next,
}

impl Workspace {
    /// Put the block `toward` the one in front in front. While one is
    /// maximized, that one is shown instead.
    pub fn focus_block(&mut self, toward: Toward, cx: &mut Context<Self>) {
        let Some((_, to)) = self.step(toward, cx) else {
            return;
        };
        let id = self.blocks[to].view.entity_id();
        self.bring_block(id, cx);
    }

    /// Put the block clicked or put in front from the keyboard last, before
    /// the one in front, in front: twice over, the one in front first.
    pub fn recent_block(&mut self, cx: &mut Context<Self>) {
        let blocks = self.block_ids();
        let front = self.active_view().map(|view| view.entity_id());
        if let Some(id) = self.block_history.recent(front, |id| blocks.contains(&id)) {
            self.bring_block(id, cx);
        }
    }

    /// Put the block in front before the one in front in front; run again,
    /// with nothing else run between, the one before that, and so on,
    /// passing those it has been to.
    pub fn back_block(&mut self, cx: &mut Context<Self>) {
        let blocks = self.block_ids();
        let front = self.active_view().map(|view| view.entity_id());
        let walking = api::ran_before(cx) == Some("block.back");
        if let Some(id) = self
            .block_history
            .back(front, walking, |id| blocks.contains(&id))
        {
            self.bring_block(id, cx);
        }
    }

    /// The block `id` was clicked, or put in front from the keyboard: not
    /// as the mouse passes over it, on its way to another.
    pub(super) fn note_front_block(&mut self, id: EntityId) {
        let blocks = self.block_ids();
        self.block_history
            .note(id, |other| !blocks.contains(&other));
    }

    /// Put the block `id` in front from the keyboard, with a ring; while
    /// one is maximized, that one is shown instead.
    pub fn bring_block(&mut self, id: EntityId, cx: &mut Context<Self>) {
        if self.maximized.is_some() {
            self.maximized = Some(id);
        }
        self.active = Some(id);
        self.keyboard_focus = true;
        self.note_front_block(id);
        self.blocks_changed(cx);
        refresh_menus(cx);
    }

    fn block_ids(&self) -> Vec<EntityId> {
        self.blocks
            .iter()
            .map(|block| block.view.entity_id())
            .collect()
    }

    /// Swap the block in front with the one `toward` it, staying in front.
    /// Not while one is maximized, where it wouldn't be seen.
    pub fn move_front_block(&mut self, toward: Toward, cx: &mut Context<Self>) {
        if self.maximized.is_some() {
            return;
        }
        let Some((from, to)) = self.step(toward, cx) else {
            return;
        };
        let id = self.blocks[from].view.entity_id();
        self.blocks.swap(from, to);
        self.active = Some(id);
        self.keyboard_focus = true;
        self.blocks_changed(cx);
        refresh_menus(cx);
    }

    /// Where the block in front is, and the block `toward` it, if there's one
    /// there.
    fn step(&self, toward: Toward, cx: &App) -> Option<(usize, usize)> {
        let front = self.active_view()?.entity_id();
        let from = self
            .blocks
            .iter()
            .position(|t| t.view.entity_id() == front)?;
        let count = self.blocks.len();
        let to = neighbor(from, count, columns(count, cx), toward)?;
        Some((from, to))
    }
}

/// The place `toward` place `index` among `count` blocks in rows of
/// `columns`, as the grid lays them out, if there's a block there: across a
/// row and down a column, stopping at the edges, with the last block for
/// the one below in a last row too short to have it; or round the
/// window's order, back to the start after the last.
fn neighbor(index: usize, count: usize, columns: usize, toward: Toward) -> Option<usize> {
    if index >= count {
        return None;
    }
    let columns = columns.max(1);
    let (row, column) = (index / columns, index % columns);
    let last_row = (count - 1) / columns;
    let to = match toward {
        Toward::Left => (column > 0).then(|| index - 1),
        Toward::Right => (column + 1 < columns && index + 1 < count).then(|| index + 1),
        Toward::Up => (row > 0).then(|| index - columns),
        Toward::Down => (row < last_row).then(|| (index + columns).min(count - 1)),
        Toward::Previous => Some((index + count - 1) % count),
        Toward::Next => Some((index + 1) % count),
    };
    to.filter(|&to| to != index)
}

#[cfg(test)]
#[path = "tests/focus.rs"]
mod tests;
