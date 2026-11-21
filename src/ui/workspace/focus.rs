//! Going from tile to tile, and moving tiles about the grid, from the
//! keyboard: to the tile beside, above or below the one in front, or the
//! one before or after it in the window's order. The tile focused so shows
//! a ring until the mouse moves, as the tile under the mouse is in front
//! from then on.

use gpui::{App, Context, EntityId};

use super::Workspace;
use super::grid::columns;
use crate::api;
use crate::app::refresh_menus;

/// Where to go from the tile in front.
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
    /// Put the tile `toward` the one in front in front. While one is
    /// maximized, that one is shown instead.
    pub fn focus_tile(&mut self, toward: Toward, cx: &mut Context<Self>) {
        let Some((_, to)) = self.step(toward, cx) else {
            return;
        };
        let id = self.tiles[to].view.entity_id();
        self.bring_tile(id, cx);
    }

    /// Put the tile clicked or put in front from the keyboard last, before
    /// the one in front, in front: twice over, the one in front first.
    pub fn recent_tile(&mut self, cx: &mut Context<Self>) {
        let tiles = self.tile_ids();
        let front = self.active_view().map(|view| view.entity_id());
        if let Some(id) = self.tile_history.recent(front, |id| tiles.contains(&id)) {
            self.bring_tile(id, cx);
        }
    }

    /// Put the tile in front before the one in front in front; run again,
    /// with nothing else run between, the one before that, and so on,
    /// passing those it has been to.
    pub fn back_tile(&mut self, cx: &mut Context<Self>) {
        let tiles = self.tile_ids();
        let front = self.active_view().map(|view| view.entity_id());
        let walking = api::ran_before(cx) == Some("tile.back");
        if let Some(id) = self
            .tile_history
            .back(front, walking, |id| tiles.contains(&id))
        {
            self.bring_tile(id, cx);
        }
    }

    /// The tile `id` was clicked, or put in front from the keyboard: not
    /// as the mouse passes over it, on its way to another.
    pub(super) fn note_front_tile(&mut self, id: EntityId) {
        let tiles = self.tile_ids();
        self.tile_history.note(id, |other| !tiles.contains(&other));
    }

    /// Put the tile `id` in front from the keyboard, with a ring; while
    /// one is maximized, that one is shown instead.
    fn bring_tile(&mut self, id: EntityId, cx: &mut Context<Self>) {
        if self.maximized.is_some() {
            self.maximized = Some(id);
        }
        self.active = Some(id);
        self.keyboard_focus = true;
        self.note_front_tile(id);
        self.tiles_changed(cx);
        refresh_menus(cx);
    }

    fn tile_ids(&self) -> Vec<EntityId> {
        self.tiles
            .iter()
            .map(|tile| tile.view.entity_id())
            .collect()
    }

    /// Swap the tile in front with the one `toward` it, staying in front.
    /// Not while one is maximized, where it wouldn't be seen.
    pub fn move_front_tile(&mut self, toward: Toward, cx: &mut Context<Self>) {
        if self.maximized.is_some() {
            return;
        }
        let Some((from, to)) = self.step(toward, cx) else {
            return;
        };
        let id = self.tiles[from].view.entity_id();
        self.tiles.swap(from, to);
        self.active = Some(id);
        self.keyboard_focus = true;
        self.tiles_changed(cx);
        refresh_menus(cx);
    }

    /// Where the tile in front is, and the tile `toward` it, if there's one
    /// there.
    fn step(&self, toward: Toward, cx: &App) -> Option<(usize, usize)> {
        let front = self.active_view()?.entity_id();
        let from = self
            .tiles
            .iter()
            .position(|t| t.view.entity_id() == front)?;
        let count = self.tiles.len();
        let to = neighbor(from, count, columns(count, cx), toward)?;
        Some((from, to))
    }
}

/// The place `toward` place `index` among `count` tiles in rows of
/// `columns`, as the grid lays them out, if there's a tile there: across a
/// row and down a column, stopping at the edges, with the last tile for
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
mod tests {
    use super::Toward::*;
    use super::*;

    /// From each place, in order, where `toward` goes.
    fn from_each(count: usize, columns: usize, toward: Toward) -> Vec<Option<usize>> {
        (0..count)
            .map(|index| neighbor(index, count, columns, toward))
            .collect()
    }

    #[test]
    fn across_and_down_a_full_grid_stopping_at_its_edges() {
        // 0 1
        // 2 3
        assert_eq!(from_each(4, 2, Left), [None, Some(0), None, Some(2)]);
        assert_eq!(from_each(4, 2, Right), [Some(1), None, Some(3), None]);
        assert_eq!(from_each(4, 2, Up), [None, None, Some(0), Some(1)]);
        assert_eq!(from_each(4, 2, Down), [Some(2), Some(3), None, None]);
    }

    #[test]
    fn down_into_a_short_last_row_goes_to_its_last_tile() {
        // 0 1 2
        // 3 4
        assert_eq!(
            from_each(5, 3, Down),
            [Some(3), Some(4), Some(4), None, None]
        );
        assert_eq!(
            from_each(5, 3, Right),
            [Some(1), Some(2), None, Some(4), None]
        );
        assert_eq!(from_each(5, 3, Up), [None, None, None, Some(0), Some(1)]);
    }

    #[test]
    fn previous_and_next_go_round_the_order() {
        assert_eq!(from_each(3, 2, Next), [Some(1), Some(2), Some(0)]);
        assert_eq!(from_each(3, 2, Previous), [Some(2), Some(0), Some(1)]);
    }

    #[test]
    fn one_tile_or_one_row_goes_nowhere_it_has_none() {
        for toward in [Left, Right, Up, Down, Previous, Next] {
            assert_eq!(from_each(1, 1, toward), [None], "{toward:?}");
        }
        // More columns than tiles: one row.
        assert_eq!(from_each(2, 4, Down), [None, None]);
        assert_eq!(from_each(2, 4, Right), [Some(1), None]);
        assert_eq!(neighbor(0, 0, 2, Next), None);
    }
}
